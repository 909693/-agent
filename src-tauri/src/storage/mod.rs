use std::fs;
use std::path::PathBuf;
use std::sync::RwLock;

use serde_json::Value;

static CUSTOM_DATA_DIR: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Default data directory
fn default_data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("retl")
        .join("projects")
}

/// Config file that persists the custom data dir setting
fn config_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("retl")
        .join("config.json")
}

/// Load custom dir from config on startup
pub fn init_data_dir() {
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

pub fn project_dir(project_id: &str) -> PathBuf {
    let dir = data_dir().join(project_id);
    fs::create_dir_all(&dir).ok();
    dir
}

pub fn save_json(project_id: &str, filename: &str, data: &Value) -> Result<(), String> {
    let path = project_dir(project_id).join(filename);
    let content = serde_json::to_string_pretty(data).map_err(|e| e.to_string())?;
    fs::write(path, content).map_err(|e| e.to_string())
}

pub fn load_json(project_id: &str, filename: &str) -> Result<Option<Value>, String> {
    let path = project_dir(project_id).join(filename);
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
        "word_count": text.len(),
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
    let path = project_dir(project_id).join("snapshots").join(snapshot_file);
    if !path.exists() {
        return Err("快照不存在".into());
    }
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}
