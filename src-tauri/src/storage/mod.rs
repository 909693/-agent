mod keyring;
mod audit;

use std::fs;
use std::path::PathBuf;
use std::sync::RwLock;
use std::sync::{Arc, Mutex, LazyLock};
use std::sync::atomic::{AtomicU64, Ordering};
use std::collections::HashMap;

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

/// Monotonic counter for unique temp-file / snapshot names, so concurrent atomic
/// writes to the same target never clobber each other's temp file.
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Per-project locks serializing read-modify-write sequences on shared aggregate
/// JSON files (e.g. chapter_summaries.json) to prevent lost updates.
static PROJECT_LOCKS: LazyLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn unique_tmp_name(base: &str) -> String {
    let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(".{}.{}.{}.tmp", base, std::process::id(), n)
}

/// Acquire the per-project lock. Hold the returned guard across a
/// load → modify → save sequence to serialize concurrent writers.
pub fn project_lock(project_id: &str) -> Arc<Mutex<()>> {
    let mut map = PROJECT_LOCKS.lock().unwrap_or_else(|e| e.into_inner());
    map.entry(project_id.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

/// Get the directory where the executable lives
fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

/// Standard app data root: ~/Library/Application Support/retl (macOS) or
/// %LOCALAPPDATA%/retl (Windows). Everything lives here — projects, LLM config,
/// secrets, extensions — so replacing the .app bundle never touches user data.
/// (An earlier design kept data next to the executable, portable-style; on macOS
/// a Finder "replace" of the .app deleted it all. See migrate_portable_data.)
pub(crate) fn standard_root() -> PathBuf {
    dirs::data_local_dir()
        .map(|d| d.join("retl"))
        .unwrap_or_else(|| exe_dir().join("data"))
}

/// Default data directory: <standard_root>/projects/
fn default_data_dir() -> PathBuf {
    standard_root().join("projects")
}

/// Config file: <standard_root>/config.json
fn config_path() -> PathBuf {
    standard_root().join("config.json")
}

/// One-time recovery migration: earlier versions stored ALL data inside the app
/// bundle (<exe_dir>/data), which a Finder "replace .app" install wipes. If that
/// directory still exists, move its contents to the standard root — bundle files
/// WIN over same-name files at the standard root, because the bundle copy was the
/// live, most-recently-written data (the standard-root copy, if any, is a stale
/// pre-portable leftover). Afterwards the bundle dir is renamed to data.migrated
/// as a belt-and-suspenders backup; if the rename fails the migration simply
/// re-runs (idempotent overwrite-copy) on next launch.
fn migrate_portable_data() {
    let old_root = exe_dir().join("data");
    if !old_root.exists() {
        return;
    }
    let new_root = standard_root();
    if new_root == old_root {
        // dirs::data_local_dir() unavailable — nothing sane to migrate to.
        return;
    }

    eprintln!("[Storage] Migrating portable data from {:?} to {:?}", old_root, new_root);
    fs::create_dir_all(&new_root).ok();
    copy_dir_overwrite(&old_root, &new_root);

    // If a custom data_dir points inside the (doomed) bundle dir, drop it so the
    // app falls back to the standard projects dir where the data now lives.
    let cfg = new_root.join("config.json");
    if let Ok(content) = fs::read_to_string(&cfg) {
        if let Ok(mut val) = serde_json::from_str::<Value>(&content) {
            let points_into_bundle = val["data_dir"].as_str()
                .map(|d| PathBuf::from(d).starts_with(&old_root))
                .unwrap_or(false);
            if points_into_bundle {
                val["data_dir"] = Value::String(String::new());
                let _ = fs::write(&cfg, serde_json::to_string_pretty(&val).unwrap_or_default());
            }
        }
    }

    // Keep the bundle copy as a backup under a name that won't re-trigger migration.
    let backup = exe_dir().join("data.migrated");
    if backup.exists() {
        let _ = fs::remove_dir_all(&backup);
    }
    if fs::rename(&old_root, &backup).is_err() {
        eprintln!("[Storage] Could not rename old data dir (will re-migrate next launch)");
    }
    eprintln!("[Storage] Portable data migration complete");
}

/// Recursive copy where src files OVERWRITE dst files (used by the portable→
/// standard migration, where src is the newer live data).
fn copy_dir_overwrite(src: &std::path::Path, dst: &std::path::Path) {
    fs::create_dir_all(dst).ok();
    if let Ok(entries) = fs::read_dir(src) {
        for entry in entries.flatten() {
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());
            if src_path.is_dir() {
                copy_dir_overwrite(&src_path, &dst_path);
            } else {
                fs::copy(&src_path, &dst_path).ok();
            }
        }
    }
}

/// Load custom dir from config on startup
pub fn init_data_dir() {
    migrate_portable_data();
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
    // Atomic write: write to a UNIQUE temp file then rename. A unique name is
    // essential — a fixed `.name.tmp` lets two concurrent writers to the same
    // target clobber each other's temp file (corruption / spurious ENOENT).
    let tmp_path = dir.join(unique_tmp_name(&safe_filename));
    fs::write(&tmp_path, &content).map_err(|e| format!("写入临时文件失败: {}", e))?;
    // On Windows, fs::rename fails if destination exists; remove it first.
    // On POSIX, rename atomically replaces, so we must NOT pre-remove (that would
    // create a window where a concurrent reader sees no file).
    #[cfg(windows)]
    {
        if path.exists() {
            let _ = fs::remove_file(&path);
        }
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
    // Millisecond timestamp + monotonic seq so two snapshots of the same chapter
    // within the same second (e.g. autosave racing a manual save) never collide.
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S_%3f").to_string();
    let seq = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let filename = format!("chapter_{:03}_{}_{}.json", chapter_number, timestamp, seq);
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

/// Swap the snapshot files of two chapters. Snapshots are named by chapter number
/// (`chapter_{n:03}_...json`), so when a reorder swaps two chapters' text this
/// keeps each chapter's version history following its content. Uses a dot-prefixed
/// temp name (never matched by list_snapshots) for the three-step rename.
pub fn swap_snapshots(project_id: &str, a: u32, b: u32) -> Result<(), String> {
    if a == b {
        return Ok(());
    }
    let snap_dir = project_dir(project_id).join("snapshots");
    if !snap_dir.exists() {
        return Ok(());
    }
    let prefix_a = format!("chapter_{:03}_", a);
    let prefix_b = format!("chapter_{:03}_", b);
    // Unique per-call token so a leftover temp file (e.g. from a crash mid-swap)
    // can never be picked up by a later swap — pid alone can be reused.
    let token = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_prefix = format!(".swaptmp_{}_{}_", std::process::id(), token);

    let entries: Vec<String> = match fs::read_dir(&snap_dir) {
        Ok(rd) => rd.flatten().filter_map(|e| e.file_name().to_str().map(String::from)).collect(),
        Err(_) => return Ok(()),
    };
    // Step 1: a's snapshots → temp. Record exactly what we moved so step 3 only
    // touches this call's files (never a stray `.swaptmp_` orphan).
    let mut moved: Vec<String> = Vec::new();
    for name in &entries {
        if let Some(rest) = name.strip_prefix(&prefix_a) {
            if fs::rename(snap_dir.join(name), snap_dir.join(format!("{}{}", tmp_prefix, rest))).is_ok() {
                moved.push(rest.to_string());
            }
        }
    }
    // Step 2: b's snapshots → a
    for name in &entries {
        if let Some(rest) = name.strip_prefix(&prefix_b) {
            let _ = fs::rename(snap_dir.join(name), snap_dir.join(format!("{}{}", prefix_a, rest)));
        }
    }
    // Step 3: temp (originally a's) → b, only the files this call moved.
    for rest in &moved {
        let _ = fs::rename(
            snap_dir.join(format!("{}{}", tmp_prefix, rest)),
            snap_dir.join(format!("{}{}", prefix_b, rest)),
        );
    }
    Ok(())
}

// ===== LLM Config (app-level, not per-project) =====

/// Root app data directory (LLM config, secrets): <standard_root>/
fn app_root_dir() -> PathBuf {
    let dir = standard_root();
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

        // Retrieve API key from keychain for the configured provider ONLY.
        // Do NOT fall back to other providers' keys: the frontend writes the
        // returned config back on startup, which would copy provider A's key
        // into provider B's slot and could send it to the wrong endpoint.
        let api_key = keyring::get_api_key(provider)
            .ok()
            .flatten()
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

// ===== 多供应商配置(providers)=====
//
// 数据结构:llm_providers.json = { "activeId": "<uuid>", "providers": [ {..} ] }
// 每个 provider:{ id, name, apiFormat, baseUrl, model, proxyUrl?, userAgent? }
// apiKey 不写进 JSON,单独存 .secrets.json(键 = provider id)。
//
// 这是「按格式存一套」(llm_profiles.json)的升级:允许任意多个具名供应商。

fn providers_path() -> PathBuf {
    app_root_dir().join("llm_providers.json")
}

/// 保存 providers 列表(原子写)。apiKey 抽出存 .secrets.json,JSON 里不落明文。
pub fn save_llm_providers(data: &Value) -> Result<(), String> {
    let dir = app_root_dir();
    let path = providers_path();

    let mut stripped = data.clone();
    let mut valid_ids: Vec<String> = Vec::new();

    if let Some(arr) = stripped.get_mut("providers").and_then(|v| v.as_array_mut()) {
        for p in arr.iter_mut() {
            if let Some(obj) = p.as_object_mut() {
                let id = obj.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if id.is_empty() {
                    continue;
                }
                // 存 key 到 .secrets.json(占位符 / 空则跳过,保留已有 key)
                if let Some(api_key) = obj.get("apiKey").and_then(|v| v.as_str()) {
                    if !api_key.is_empty() && api_key != "***STORED_IN_KEYCHAIN***" {
                        keyring::store_api_key(&id, api_key)?;
                    }
                }
                obj.remove("apiKey");
                valid_ids.push(id);
            }
        }
    }

    // 清理被删除供应商的孤儿 key
    keyring::prune_keys(&valid_ids)?;

    let content = serde_json::to_string_pretty(&stripped).map_err(|e| e.to_string())?;
    let tmp_path = dir.join(".llm_providers.json.tmp");
    fs::write(&tmp_path, &content).map_err(|e| format!("写入临时文件失败: {}", e))?;
    if path.exists() {
        let _ = fs::remove_file(&path);
    }
    fs::rename(&tmp_path, &path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        format!("重命名文件失败: {}", e)
    })
}

/// 读取 providers 列表,并从 .secrets.json 回填各 provider 的 apiKey。
/// 若 llm_providers.json 不存在,尝试从旧配置(llm_config/llm_profiles)迁移一次。
pub fn load_llm_providers() -> Result<Option<Value>, String> {
    let path = providers_path();
    if !path.exists() {
        // 首次:从旧配置迁移
        if let Some(migrated) = migrate_to_providers()? {
            return Ok(Some(migrated));
        }
        return Ok(None);
    }
    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut val: Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;

    if let Some(arr) = val.get_mut("providers").and_then(|v| v.as_array_mut()) {
        for p in arr.iter_mut() {
            if let Some(obj) = p.as_object_mut() {
                let id = obj.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let api_key = keyring::get_api_key(&id).ok().flatten().unwrap_or_default();
                obj.insert("apiKey".to_string(), serde_json::json!(api_key));
            }
        }
    }

    Ok(Some(val))
}

/// 一次性迁移:旧 llm_config.json(当前生效)+ llm_profiles.json(每格式一套)
/// → providers 列表。当前生效配置设为 active。写入 llm_providers.json 后返回。
/// 无任何旧配置则返回 None。
fn migrate_to_providers() -> Result<Option<Value>, String> {
    let mut providers: Vec<Value> = Vec::new();
    let mut active_id = String::new();

    // 1) 当前生效配置(llm_config.json)→ 第一个供应商,设为 active
    if let Ok(Some(cfg)) = load_llm_config() {
        let base_url = cfg.get("baseUrl").and_then(|v| v.as_str()).unwrap_or("");
        let model = cfg.get("model").and_then(|v| v.as_str()).unwrap_or("");
        let api_format = cfg.get("apiFormat").and_then(|v| v.as_str()).unwrap_or("openai");
        let api_key = cfg.get("apiKey").and_then(|v| v.as_str()).unwrap_or("");
        // 有意义才迁移(至少填过 url 或 model)
        if !base_url.is_empty() || !model.is_empty() {
            let id = uuid_v4();
            active_id = id.clone();
            if !api_key.is_empty() {
                let _ = keyring::store_api_key(&id, api_key);
            }
            providers.push(serde_json::json!({
                "id": id,
                "name": provider_display_name(api_format, base_url),
                "apiFormat": api_format,
                "baseUrl": base_url,
                "model": model,
                "proxyUrl": cfg.get("proxyUrl").and_then(|v| v.as_str()).unwrap_or(""),
                "userAgent": cfg.get("userAgent").and_then(|v| v.as_str()).unwrap_or(""),
            }));
        }
    }

    // 2) llm_profiles.json 里与当前生效不同的其它套 → 追加为供应商
    if let Ok(Some(profiles)) = load_llm_profiles() {
        if let Some(obj) = profiles.as_object() {
            for (format_name, profile) in obj.iter() {
                let base_url = profile.get("baseUrl").and_then(|v| v.as_str()).unwrap_or("");
                let model = profile.get("model").and_then(|v| v.as_str()).unwrap_or("");
                let api_key = profile.get("apiKey").and_then(|v| v.as_str()).unwrap_or("");
                if base_url.is_empty() && model.is_empty() {
                    continue;
                }
                // 跳过与已迁移的当前配置完全相同的(format+url+model)
                let dup = providers.iter().any(|p| {
                    p.get("apiFormat").and_then(|v| v.as_str()) == Some(format_name.as_str())
                        && p.get("baseUrl").and_then(|v| v.as_str()) == Some(base_url)
                        && p.get("model").and_then(|v| v.as_str()) == Some(model)
                });
                if dup {
                    continue;
                }
                let id = uuid_v4();
                if !api_key.is_empty() {
                    let _ = keyring::store_api_key(&id, api_key);
                }
                providers.push(serde_json::json!({
                    "id": id,
                    "name": provider_display_name(format_name, base_url),
                    "apiFormat": format_name,
                    "baseUrl": base_url,
                    "model": model,
                    "proxyUrl": "",
                    "userAgent": "",
                }));
            }
        }
    }

    if providers.is_empty() {
        return Ok(None);
    }
    if active_id.is_empty() {
        active_id = providers[0].get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    }

    let result = serde_json::json!({ "activeId": active_id, "providers": providers });
    // 落盘(save 会再次抽 key,但 key 已在 .secrets.json,占位/空跳过,无副作用)
    save_llm_providers(&result)?;
    // 回填 key 后返回给前端
    load_llm_providers()
}

/// 由格式 + URL 生成一个可读的默认供应商名(如 "anthropic · api.example.com")。
fn provider_display_name(api_format: &str, base_url: &str) -> String {
    let host = base_url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or("")
        .to_string();
    if host.is_empty() {
        api_format.to_string()
    } else {
        format!("{} · {}", api_format, host)
    }
}

/// 生成新 provider id。复用 crate 已有的 uuid 依赖。
fn uuid_v4() -> String {
    uuid::Uuid::new_v4().to_string()
}
