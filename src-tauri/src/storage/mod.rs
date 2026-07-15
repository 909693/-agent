mod keyring;
mod audit;

use std::fs;
use std::path::PathBuf;
use std::sync::RwLock;

use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

#[allow(unused_imports)]
#[allow(unused_imports)]
pub use keyring::{store_api_key, get_api_key};
#[allow(unused_imports)]
pub use audit::{
    log_delete_project, log_export_project, log_mcp_start, log_mcp_stop,
    log_skill_install, log_skill_remove, log_data_dir_change,
};

static CUSTOM_DATA_DIR: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Get the directory where the executable lives
fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

/// Default data directory: <exe_dir>/data/projects/
fn default_data_dir() -> PathBuf {
    exe_dir().join("data").join("projects")
}

/// Config file: <exe_dir>/data/config.json
fn config_path() -> PathBuf {
    exe_dir().join("data").join("config.json")
}

/// Legacy data directory (pre-migration): ~/Library/Application Support/retl/ or %LOCALAPPDATA%/retl/
fn legacy_data_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("retl"))
}

/// Migrate data from legacy location to new exe-relative location
fn migrate_legacy_data() {
    let old_root = match legacy_data_dir() {
        Some(d) if d.exists() => d,
        _ => return,
    };
    let new_root = exe_dir().join("data");

    // Only migrate if new location is empty/missing and old has content
    let new_has_data = new_root.join("projects").exists()
        && fs::read_dir(new_root.join("projects")).map(|mut d| d.next().is_some()).unwrap_or(false);
    if new_has_data {
        return;
    }

    eprintln!("[Storage] Migrating legacy data from {:?} to {:?}", old_root, new_root);
    fs::create_dir_all(&new_root).ok();

    // Copy top-level files (llm_config.json, llm_profiles.json, config.json, etc.)
    if let Ok(entries) = fs::read_dir(&old_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            if path.is_file() {
                let dest = new_root.join(&name);
                if !dest.exists() {
                    fs::copy(&path, &dest).ok();
                }
            }
        }
    }

    // Copy projects/ directory recursively
    let old_projects = old_root.join("projects");
    if old_projects.exists() {
        copy_dir_recursive(&old_projects, &new_root.join("projects"));
    }

    // Copy extensions/ directory recursively (skills, mcp)
    let old_extensions = old_root.join("extensions");
    if old_extensions.exists() {
        copy_dir_recursive(&old_extensions, &new_root.join("extensions"));
    }

    eprintln!("[Storage] Migration complete");
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) {
    fs::create_dir_all(dst).ok();
    if let Ok(entries) = fs::read_dir(src) {
        for entry in entries.flatten() {
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());
            if src_path.is_dir() {
                copy_dir_recursive(&src_path, &dst_path);
            } else if !dst_path.exists() {
                fs::copy(&src_path, &dst_path).ok();
            }
        }
    }
}

/// Load custom dir from config on startup
pub fn init_data_dir() {
    migrate_legacy_data();
    let cfg = config_path();
    if cfg.exists() {
        if let Ok(content) = fs::read_to_string(&cfg) {
            if let Ok(val) = serde_json::from_str::<Value>(&content) {
                if let Some(dir) = val["data_dir"].as_str() {
                    if !dir.is_empty() {
                        let p = PathBuf::from(dir);
                        if p.exists() || fs::create_dir_all(&p).is_ok() {
                            if let Ok(mut w) = CUSTOM_DATA_DIR.write() {
                                *w = Some(p);
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn data_dir() -> PathBuf {
    let dir = if let Ok(r) = CUSTOM_DATA_DIR.read() {
        r.clone().unwrap_or_else(default_data_dir)
    } else {
        default_data_dir()
    };
    fs::create_dir_all(&dir).ok();
    dir
}

/// Set a new custom data directory. Returns Ok(()) on success.
pub fn set_custom_data_dir(new_dir: &str) -> Result<(), String> {
    let old_dir = data_dir().to_string_lossy().to_string();
    audit::log_data_dir_change(&old_dir, new_dir);

    let p = PathBuf::from(new_dir);
    fs::create_dir_all(&p).map_err(|e| format!("无法创建目录: {}", e))?;

    // Persist to config
    let cfg = config_path();
    if let Some(parent) = cfg.parent() {
        fs::create_dir_all(parent).ok();
    }
    let val = serde_json::json!({"data_dir": new_dir});
    fs::write(&cfg, serde_json::to_string_pretty(&val).unwrap_or_default())
        .map_err(|e| format!("无法保存配置: {}", e))?;

    // Update runtime
    if let Ok(mut w) = CUSTOM_DATA_DIR.write() {
        *w = Some(p);
    }
    Ok(())
}

/// Move all project data from old dir to new dir
pub fn migrate_data(old_dir: &str, new_dir: &str) -> Result<u32, String> {
    let src = PathBuf::from(old_dir);
    let dst = PathBuf::from(new_dir);
    if !src.exists() {
        return Ok(0);
    }
    fs::create_dir_all(&dst).map_err(|e| format!("无法创建目标目录: {}", e))?;

    let mut count = 0u32;
    let entries = fs::read_dir(&src).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            let target = dst.join(&name);
            if !target.exists() {
                // Copy directory recursively
                copy_dir_all(&path, &target).map_err(|e| format!("迁移失败: {}", e))?;
                count += 1;
            }
        }
    }
    Ok(count)
}

fn copy_dir_all(src: &PathBuf, dst: &PathBuf) -> Result<(), std::io::Error> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_all(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Sanitize filename to prevent path traversal
/// Enhanced with URL decoding, Unicode normalization, and strict whitelist
fn sanitize_filename(name: &str) -> Result<String, String> {
    if name.is_empty() {
        return Err("文件名不能为空".into());
    }

    // URL decode to prevent %2e%2e bypass
    let decoded = urlencoding::decode(name)
        .map_err(|_| "无效的 URL 编码".to_string())?;

    // Unicode normalization (NFC) to prevent Unicode bypass
    let normalized: String = decoded.nfc().collect();

    // Strict whitelist: only alphanumeric, dash, underscore, dot
    let is_valid = normalized.chars().all(|c: char| {
        c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'
    });

    if !is_valid {
        return Err(format!("文件名包含非法字符: {}", normalized));
    }

    // Prevent path traversal
    if normalized.contains("..") || normalized.starts_with('.') {
        return Err("检测到路径遍历尝试".into());
    }

    // Length limit
    if normalized.len() > 255 {
        return Err("文件名过长".into());
    }

    Ok(normalized)
}

pub fn project_dir(project_id: &str) -> PathBuf {
    // Sanitize project_id using the enhanced sanitize_filename
    let safe_id = match sanitize_filename(project_id) {
        Ok(id) => id,
        Err(_) => {
            // Fallback to a deterministic hash for invalid IDs
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            project_id.hash(&mut hasher);
            format!("_invalid_{:x}", hasher.finish())
        }
    };
    let dir = data_dir().join(safe_id);
    fs::create_dir_all(&dir).ok();
    dir
}

pub fn save_json(project_id: &str, filename: &str, data: &Value) -> Result<(), String> {
    let safe_filename = sanitize_filename(filename)?;
    let dir = project_dir(project_id);
    let path = dir.join(&safe_filename);
    let content = serde_json::to_string_pretty(data).map_err(|e| e.to_string())?;
    // Atomic write: write to temp file then rename to prevent corruption
    let tmp_path = dir.join(format!(".{}.tmp", safe_filename));
    fs::write(&tmp_path, &content).map_err(|e| format!("写入临时文件失败: {}", e))?;
    // On Windows, fs::rename fails if destination exists; remove it first
    if path.exists() {
        let _ = fs::remove_file(&path);
    }
    fs::rename(&tmp_path, &path).map_err(|e| {
        // Cleanup tmp on rename failure
        let _ = fs::remove_file(&tmp_path);
        format!("重命名文件失败: {}", e)
    })
}

pub fn load_json(project_id: &str, filename: &str) -> Result<Option<Value>, String> {
    let safe_filename = sanitize_filename(filename)?;
    let path = project_dir(project_id).join(&safe_filename);
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let val: Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    Ok(Some(val))
}

pub fn list_projects() -> Result<Vec<Value>, String> {
    let dir = data_dir();
    let mut projects = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                let meta_path = entry.path().join("meta.json");
                if meta_path.exists() {
                    if let Ok(content) = fs::read_to_string(&meta_path) {
                        if let Ok(val) = serde_json::from_str::<Value>(&content) {
                            projects.push(val);
                        }
                    }
                }
            }
        }
    }
    projects.sort_by(|a, b| {
        let ta = a["created_at"].as_str().unwrap_or("");
        let tb = b["created_at"].as_str().unwrap_or("");
        tb.cmp(ta)
    });
    Ok(projects)
}

pub fn delete_project(project_id: &str) -> Result<(), String> {
    audit::log_delete_project(project_id);
    let dir = project_dir(project_id);
    if dir.exists() {
        fs::remove_dir_all(dir).map_err(|e| e.to_string())
    } else {
        Err("Project not found".into())
    }
}

// ===== Snapshot System =====

/// Save a snapshot of the current chapter before overwriting
pub fn save_snapshot(project_id: &str, chapter_number: u32, text: &str) -> Result<(), String> {
    let snap_dir = project_dir(project_id).join("snapshots");
    fs::create_dir_all(&snap_dir).map_err(|e| e.to_string())?;
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let filename = format!("chapter_{:03}_{}.json", chapter_number, timestamp);
    let data = serde_json::json!({
        "text": text,
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "word_count": text.chars().count(),
        "chapter_number": chapter_number,
    });
    let content = serde_json::to_string_pretty(&data).map_err(|e| e.to_string())?;
    fs::write(snap_dir.join(filename), content).map_err(|e| e.to_string())
}

/// List snapshots for a chapter (metadata only, sorted newest first)
pub fn list_snapshots(project_id: &str, chapter_number: u32) -> Result<Vec<Value>, String> {
    let snap_dir = project_dir(project_id).join("snapshots");
    let prefix = format!("chapter_{:03}_", chapter_number);
    let mut snapshots = Vec::new();
    if let Ok(entries) = fs::read_dir(&snap_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(&prefix) && name.ends_with(".json") {
                if let Ok(content) = fs::read_to_string(entry.path()) {
                    if let Ok(val) = serde_json::from_str::<Value>(&content) {
                        snapshots.push(serde_json::json!({
                            "file": name,
                            "timestamp": val["timestamp"],
                            "word_count": val["word_count"],
                        }));
                    }
                }
            }
        }
    }
    snapshots.sort_by(|a, b| {
        let ta = a["timestamp"].as_str().unwrap_or("");
        let tb = b["timestamp"].as_str().unwrap_or("");
        tb.cmp(ta)
    });
    Ok(snapshots)
}

/// Load full snapshot content
pub fn load_snapshot(project_id: &str, snapshot_file: &str) -> Result<Value, String> {
    let safe_filename = sanitize_filename(snapshot_file)?;
    let path = project_dir(project_id).join("snapshots").join(&safe_filename);
    if !path.exists() {
        return Err("快照不存在".into());
    }
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

// ===== LLM Config (app-level, not per-project) =====

/// Root app data directory: <exe_dir>/data/
fn app_root_dir() -> PathBuf {
    let dir = exe_dir().join("data");
    fs::create_dir_all(&dir).ok();
    dir
}

/// Save LLM config to llm_config.json (atomic write)
/// API keys are stored in system keychain, not in the JSON file
pub fn save_llm_config(config: &Value) -> Result<(), String> {
    let dir = app_root_dir();
    let path = dir.join("llm_config.json");

    // Extract and store API key in keychain if present
    if let Some(api_key) = config["apiKey"].as_str() {
        if !api_key.is_empty() {
            let provider = config["apiFormat"].as_str().unwrap_or("default");
            keyring::store_api_key(provider, api_key)?;
        }
    }

    // Remove API key from JSON before saving
    let mut config_without_key = config.clone();
    if let Some(obj) = config_without_key.as_object_mut() {
        obj.insert("apiKey".to_string(), serde_json::json!("***STORED_IN_KEYCHAIN***"));
    }

    let content = serde_json::to_string_pretty(&config_without_key).map_err(|e| e.to_string())?;
    let tmp_path = dir.join(".llm_config.json.tmp");
    fs::write(&tmp_path, &content).map_err(|e| format!("写入临时文件失败: {}", e))?;
    if path.exists() {
        let _ = fs::remove_file(&path);
    }
    fs::rename(&tmp_path, &path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        format!("重命名文件失败: {}", e)
    })
}

/// Load LLM config from llm_config.json
/// API key is retrieved from system keychain
pub fn load_llm_config() -> Result<Option<Value>, String> {
    let path = app_root_dir().join("llm_config.json");
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut val: Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;

    // Retrieve API key from keychain
    if let Some(obj) = val.as_object_mut() {
        let provider = obj.get("apiFormat")
            .and_then(|v| v.as_str())
            .unwrap_or("default");

        // Try current provider first, then try all known providers
        let api_key = keyring::get_api_key(provider)
            .ok()
            .flatten()
            .or_else(|| keyring::get_api_key("openai").ok().flatten())
            .or_else(|| keyring::get_api_key("anthropic").ok().flatten())
            .or_else(|| keyring::get_api_key("gemini").ok().flatten())
            .unwrap_or_default();

        obj.insert("apiKey".to_string(), serde_json::json!(api_key));
    }

    Ok(Some(val))
}

/// Save LLM profiles (per-format configs) to llm_profiles.json (atomic write)
/// API keys within profiles are stored in system keychain, not in the JSON file
pub fn save_llm_profiles(profiles: &Value) -> Result<(), String> {
    let dir = app_root_dir();
    let path = dir.join("llm_profiles.json");

    // Store API keys in keychain and strip from JSON
    let mut profiles_without_keys = profiles.clone();
    if let Some(obj) = profiles_without_keys.as_object_mut() {
        for (format_name, profile) in obj.iter_mut() {
            if let Some(profile_obj) = profile.as_object_mut() {
                if let Some(api_key) = profile_obj.get("apiKey").and_then(|v| v.as_str()) {
                    if !api_key.is_empty() && api_key != "***STORED_IN_KEYCHAIN***" {
                        let keyring_id = format!("profile_{}", format_name);
                        keyring::store_api_key(&keyring_id, api_key)?;
                    }
                }
                profile_obj.insert("apiKey".to_string(), serde_json::json!("***STORED_IN_KEYCHAIN***"));
            }
        }
    }

    let content = serde_json::to_string_pretty(&profiles_without_keys).map_err(|e| e.to_string())?;
    let tmp_path = dir.join(".llm_profiles.json.tmp");
    fs::write(&tmp_path, &content).map_err(|e| format!("写入临时文件失败: {}", e))?;
    if path.exists() {
        let _ = fs::remove_file(&path);
    }
    fs::rename(&tmp_path, &path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        format!("重命名文件失败: {}", e)
    })
}

/// Load LLM profiles from llm_profiles.json
/// API keys are retrieved from system keychain
pub fn load_llm_profiles() -> Result<Option<Value>, String> {
    let path = app_root_dir().join("llm_profiles.json");
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut val: Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;

    // Restore API keys from keychain
    if let Some(obj) = val.as_object_mut() {
        for (format_name, profile) in obj.iter_mut() {
            if let Some(profile_obj) = profile.as_object_mut() {
                let keyring_id = format!("profile_{}", format_name);
                if let Ok(Some(api_key)) = keyring::get_api_key(&keyring_id) {
                    profile_obj.insert("apiKey".to_string(), serde_json::json!(api_key));
                } else {
                    profile_obj.insert("apiKey".to_string(), serde_json::json!(""));
                }
            }
        }
    }

    Ok(Some(val))
}
