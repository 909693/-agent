use std::fs::{self, OpenOptions};
use std::io::Write;
use chrono::Utc;
use super::data_dir;

/// Maximum audit log file size before rotation (10MB)
const MAX_LOG_SIZE: u64 = 10_000_000;

/// Log a sensitive operation to the audit log
pub fn log_operation(operation: &str, details: &str) {
    let timestamp = Utc::now().to_rfc3339();
    let log_entry = format!("[{}] {} - {}\n", timestamp, operation, details);

    let log_path = data_dir().join("audit.log");

    // Rotate if log file exceeds max size
    if let Ok(meta) = fs::metadata(&log_path) {
        if meta.len() > MAX_LOG_SIZE {
            let backup = data_dir().join("audit.log.1");
            let _ = fs::rename(&log_path, &backup);
        }
    }

    // Best-effort logging: don't fail the operation if logging fails
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
    {
        let _ = file.write_all(log_entry.as_bytes());
    }
}

/// Log project deletion
pub fn log_delete_project(project_id: &str) {
    log_operation("DELETE_PROJECT", project_id);
}

/// Log project export
pub fn log_export_project(project_id: &str, format: &str) {
    log_operation("EXPORT_PROJECT", &format!("{} (format: {})", project_id, format));
}

/// Log MCP server start
pub fn log_mcp_start(server_id: &str, command: &str) {
    log_operation("MCP_START", &format!("{} (command: {})", server_id, command));
}

/// Log MCP server stop
pub fn log_mcp_stop(server_id: &str) {
    log_operation("MCP_STOP", server_id);
}

/// Log skill installation
pub fn log_skill_install(repo_url: &str) {
    log_operation("SKILL_INSTALL", repo_url);
}

/// Log skill removal
pub fn log_skill_remove(skill_id: &str) {
    log_operation("SKILL_REMOVE", skill_id);
}

/// Log data directory change
pub fn log_data_dir_change(old_dir: &str, new_dir: &str) {
    log_operation("DATA_DIR_CHANGE", &format!("{} -> {}", old_dir, new_dir));
}
