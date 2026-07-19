use super::types::McpServerRecord;
use super::util::{
    extensions_root, git_clone_or_pull, mcp_registry_path, read_json_or_default, read_log_excerpt,
    remove_dir_if_exists, repo_id, repo_install_path, repo_name, write_json,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Mutex,
    thread,
    time::Duration,
};
use uuid::Uuid;

// Global process manager to hold Child handles
lazy_static::lazy_static! {
    static ref PROCESS_MANAGER: Mutex<HashMap<String, Child>> = Mutex::new(HashMap::new());
}

fn load_registry() -> Result<Vec<McpServerRecord>, String> {
    read_json_or_default(&mcp_registry_path())
}
fn save_registry(records: &[McpServerRecord]) -> Result<(), String> {
    write_json(&mcp_registry_path(), records)
}
fn log_path_for(id: &str) -> PathBuf {
    // Use whitelist: only alphanumeric, dash, underscore
    let safe_id: String = id.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    extensions_root().join("logs").join(format!("mcp-{safe_id}.log"))
}
fn slugify(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        out.push(if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            '-'
        });
    }
    out.trim_matches('-').to_string()
}
fn first_server(raw: &Value) -> Option<(Option<String>, &Value)> {
    if let Some(map) = raw.get("mcpServers").and_then(Value::as_object) {
        return map.iter().next().map(|(k, v)| (Some(k.clone()), v));
    }
    if let Some(map) = raw.get("servers").and_then(Value::as_object) {
        return map.iter().next().map(|(k, v)| (Some(k.clone()), v));
    }
    if let Some(arr) = raw.get("servers").and_then(Value::as_array) {
        return arr
            .first()
            .map(|v| (v.get("name").and_then(Value::as_str).map(String::from), v));
    }
    if raw.get("command").is_some() || raw.get("cmd").is_some() {
        return Some((
            raw.get("name").and_then(Value::as_str).map(String::from),
            raw,
        ));
    }
    None
}
/// Result type for MCP config parsing
type McpParseResult = Result<
    (
        String,
        String,
        Vec<String>,
        BTreeMap<String, String>,
        String,
    ),
    String,
>;

fn read_prefill(path: &Path, fallback: &str) -> McpParseResult {
    for file in [".mcp.json", "mcp.json"] {
        let cfg = path.join(file);
        if !cfg.exists() {
            continue;
        }
        let raw: Value =
            serde_json::from_str(&fs::read_to_string(&cfg).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        if let Some((alias, item)) = first_server(&raw) {
            let env = item
                .get("env")
                .and_then(Value::as_object)
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect()
                })
                .unwrap_or_default();
            let args = item
                .get("args")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let name = item
                .get("name")
                .and_then(Value::as_str)
                .map(String::from)
                .or(alias)
                .unwrap_or_else(|| fallback.to_string());
            let cmd = item
                .get("command")
                .or_else(|| item.get("cmd"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let cwd = item
                .get("cwd")
                .and_then(Value::as_str)
                .map(String::from)
                .unwrap_or_else(|| path.to_string_lossy().to_string());
            return Ok((name, cmd, args, env, cwd));
        }
    }

    // Infer from pyproject.toml (Python MCP servers)
    let pyproject = path.join("pyproject.toml");
    if pyproject.exists() {
        if let Ok(content) = fs::read_to_string(&pyproject) {
            let mut pkg_name = String::new();
            let mut in_project = false;
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("[project]") {
                    in_project = true;
                } else if trimmed.starts_with('[') {
                    in_project = false;
                } else if in_project && trimmed.starts_with("name") {
                    if let Some(val) = trimmed.split('=').nth(1) {
                        pkg_name = val.trim().trim_matches('"').trim_matches('\'').to_string();
                    }
                }
            }
            if !pkg_name.is_empty() {
                return Ok((
                    fallback.to_string(),
                    "uvx".to_string(),
                    vec![pkg_name],
                    BTreeMap::new(),
                    path.to_string_lossy().to_string(),
                ));
            }
        }
    }

    // Infer from package.json (Node MCP servers)
    let pkg_json = path.join("package.json");
    if pkg_json.exists() {
        if let Ok(content) = fs::read_to_string(&pkg_json) {
            if let Ok(raw) = serde_json::from_str::<Value>(&content) {
                let name = raw.get("name").and_then(Value::as_str).unwrap_or(fallback).to_string();
                return Ok((
                    name.clone(),
                    "npx".to_string(),
                    vec!["-y".to_string(), name],
                    BTreeMap::new(),
                    path.to_string_lossy().to_string(),
                ));
            }
        }
    }

    Ok((
        fallback.to_string(),
        String::new(),
        Vec::new(),
        BTreeMap::new(),
        path.to_string_lossy().to_string(),
    ))
}
fn resolved_command(record: &McpServerRecord) -> Result<String, String> {
    // Strict validation: only allow absolute paths or simple command names
    let cmd = &record.command;

    // Empty command
    if cmd.is_empty() {
        return Err("命令不能为空".into());
    }

    // URL decode to prevent %2f bypass
    let decoded = urlencoding::decode(cmd)
        .map_err(|_| "命令包含无效编码".to_string())?;
    let cmd = decoded.as_ref();

    // Path traversal check
    if cmd.contains("..") || cmd.contains('\0') {
        return Err("检测到路径遍历尝试".into());
    }

    // Block shell metacharacters to prevent command injection
    const SHELL_META: &[char] = &[';', '|', '&', '$', '(', ')', '<', '>', '`', '!', '{', '}', '*', '?', '#', '~', '\n', '\r'];
    if cmd.chars().any(|c| SHELL_META.contains(&c)) {
        return Err("命令包含非法 shell 字符".into());
    }

    // If absolute path, verify it exists and is executable
    if Path::new(cmd).is_absolute() {
        let path = Path::new(cmd);
        if !path.exists() {
            return Err(format!("可执行文件不存在: {}", cmd));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
            let permissions = metadata.permissions();
            if permissions.mode() & 0o111 == 0 {
                return Err(format!("文件不可执行: {}", cmd));
            }
        }
        return Ok(cmd.to_string());
    }

    // If simple command name (no path separators), allow it (will be resolved via PATH)
    if !cmd.contains('/') && !cmd.contains('\\') {
        return Ok(cmd.to_string());
    }

    // Relative path: resolve against cwd
    let cwd = if record.cwd.is_empty() {
        &record.install_path
    } else {
        &record.cwd
    };

    let resolved = PathBuf::from(cwd).join(cmd);
    let canonical = resolved.canonicalize()
        .map_err(|e| format!("无法解析命令路径: {}", e))?;

    // Verify resolved path is within allowed directory
    let cwd_canonical = PathBuf::from(cwd).canonicalize()
        .map_err(|e| format!("无法解析工作目录: {}", e))?;

    if !canonical.starts_with(&cwd_canonical) {
        return Err("命令路径超出允许范围".into());
    }

    Ok(canonical.to_string_lossy().to_string())
}

fn spawn_server(record: &McpServerRecord, truncate: bool) -> Result<std::process::Child, String> {
    let log_path = PathBuf::from(&record.log_path);
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let log = OpenOptions::new()
        .create(true)
        .write(true)
        .append(!truncate)
        .truncate(truncate)
        .open(&log_path)
        .map_err(|e| e.to_string())?;
    let err = log.try_clone().map_err(|e| e.to_string())?;

    // Validate and resolve command
    let resolved_cmd = resolved_command(record)?;

    let mut cmd = Command::new(resolved_cmd);
    if !record.cwd.is_empty() {
        cmd.current_dir(&record.cwd);
    } else if !record.install_path.is_empty() {
        cmd.current_dir(&record.install_path);
    }
    if !record.args.is_empty() {
        cmd.args(&record.args);
    }
    if !record.env.is_empty() {
        cmd.envs(&record.env);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(err))
        .spawn()
        .map_err(|e| e.to_string())
}
fn is_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    #[cfg(windows)]
    {
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/NH"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    }
}
fn stop_pid(pid: u32) -> Result<(), String> {
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .arg(pid.to_string())
            .status()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string()])
            .status()
            .map_err(|e| e.to_string())?;
    }

    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(3);

    while start.elapsed() < timeout {
        if !is_alive(pid) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }

    if is_alive(pid) {
        #[cfg(unix)]
        {
            let _ = Command::new("kill")
                .args(["-9", &pid.to_string()])
                .status()
                .map_err(|e| e.to_string())?;
        }
        #[cfg(windows)]
        {
            let _ = Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/F"])
                .status()
                .map_err(|e| e.to_string())?;
        }

        thread::sleep(Duration::from_millis(500));
        if is_alive(pid) {
            return Err(format!("无法终止进程 {}", pid));
        }
    }
    Ok(())
}

pub fn list_mcp_servers() -> Result<Value, String> {
    serde_json::to_value(load_registry()?).map_err(|e| e.to_string())
}
pub fn install_mcp_repo(repo_url: String) -> Result<Value, String> {
    let mut records = load_registry()?;
    let id = repo_id(&repo_url)?;
    let path = repo_install_path("mcp", &repo_url)?;
    git_clone_or_pull(&repo_url, &path)?;
    let (name, command, args, env, cwd) = read_prefill(&path, &repo_name(&repo_url)?)?;
    let log_path = log_path_for(&id).to_string_lossy().to_string();
    let mut record = McpServerRecord {
        id: id.clone(),
        name,
        repo_url,
        install_path: path.to_string_lossy().to_string(),
        command,
        args,
        env,
        cwd,
        enabled: true,
        running: false,
        pid: None,
        last_test_status: String::new(),
        log_path,
    };
    if let Some(old) = records.iter().find(|item| item.id == id).cloned() {
        record.running = old.running;
        record.pid = old.pid;
        if record.command.is_empty() {
            record.command = old.command;
        }
        if record.args.is_empty() {
            record.args = old.args;
        }
        if record.env.is_empty() {
            record.env = old.env;
        }
        if record.cwd.is_empty() {
            record.cwd = old.cwd;
        }
        if record.last_test_status.is_empty() {
            record.last_test_status = old.last_test_status;
        }
    }
    if let Some(i) = records.iter().position(|item| item.id == id) {
        records[i] = record.clone();
    } else {
        records.push(record.clone());
    }
    save_registry(&records)?;
    serde_json::to_value(record).map_err(|e| e.to_string())
}
pub fn save_mcp_server(server: Value) -> Result<Value, String> {
    let mut records = load_registry()?;
    let mut record: McpServerRecord = serde_json::from_value(server).map_err(|e| e.to_string())?;
    if record.id.is_empty() {
        record.id = if !record.repo_url.is_empty() {
            repo_id(&record.repo_url)?
        } else if !record.name.is_empty() {
            slugify(&record.name)
        } else {
            Uuid::new_v4().to_string()
        };
    }
    let existing = records.iter().find(|item| item.id == record.id).cloned();
    if record.name.is_empty() {
        record.name = existing
            .as_ref()
            .map(|v| v.name.clone())
            .or_else(|| {
                if record.repo_url.is_empty() {
                    None
                } else {
                    repo_name(&record.repo_url).ok()
                }
            })
            .unwrap_or_else(|| record.id.clone());
    }
    if record.install_path.is_empty() && !record.repo_url.is_empty() {
        record.install_path = repo_install_path("mcp", &record.repo_url)?
            .to_string_lossy()
            .to_string();
    }
    if record.cwd.is_empty() {
        record.cwd = existing
            .as_ref()
            .map(|v| v.cwd.clone())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| record.install_path.clone());
    }
    // log_path is an internal, id-derived path and must never be client-controlled:
    // a supplied absolute path would let a saved config truncate (on log open) or
    // read (via the log viewer) an arbitrary file. Always derive it from the id.
    record.log_path = log_path_for(&record.id).to_string_lossy().to_string();
    if record.command.is_empty() {
        if let Some(old) = &existing {
            record.command = old.command.clone();
            record.args = if record.args.is_empty() {
                old.args.clone()
            } else {
                record.args
            };
            record.env = if record.env.is_empty() {
                old.env.clone()
            } else {
                record.env
            };
        }
    }
    if existing.is_none() && !record.enabled {
        record.enabled = true;
    }
    if let Some(old) = &existing {
        if record.pid.is_none() && !record.running {
            record.pid = old.pid;
            record.running = old.running;
        }
        if record.last_test_status.is_empty() {
            record.last_test_status = old.last_test_status.clone();
        }
    }
    if let Some(i) = records.iter().position(|item| item.id == record.id) {
        records[i] = record.clone();
    } else {
        records.push(record.clone());
    }
    save_registry(&records)?;
    serde_json::to_value(record).map_err(|e| e.to_string())
}
pub fn delete_mcp_server(server_id: String) -> Result<(), String> {
    let mut records = load_registry()?;
    let i = records
        .iter()
        .position(|item| item.id == server_id)
        .ok_or_else(|| "MCP server not found".to_string())?;
    let record = records.remove(i);
    if let Some(pid) = record.pid {
        let _ = stop_pid(pid);
    }
    if !record.install_path.is_empty() {
        let _ = remove_dir_if_exists(Path::new(&record.install_path));
    }
    save_registry(&records)
}
pub fn test_mcp_server(server_id: String) -> Result<Value, String> {
    use std::time::Instant;
    let mut records = load_registry()?;
    let i = records
        .iter()
        .position(|item| item.id == server_id)
        .ok_or_else(|| "MCP server not found".to_string())?;
    let mut child = spawn_server(&records[i], true)?;

    // Non-blocking wait with timeout
    let start = Instant::now();
    let timeout = Duration::from_millis(1200);
    while start.elapsed() < timeout {
        if let Ok(Some(_)) = child.try_wait() {
            // Process exited (failed)
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    let status = child.try_wait().map_err(|e| e.to_string())?;
    let success = status.is_none();
    if success {
        let _ = child.kill();
        let _ = child.wait();
    }
    records[i].last_test_status = if success {
        "ok".into()
    } else {
        format!("failed: {:?}", status)
    };
    let excerpt = read_log_excerpt(Path::new(&records[i].log_path), 4000);
    save_registry(&records)?;
    Ok(json!({"success": success, "status": records[i].last_test_status, "logExcerpt": excerpt}))
}
pub fn start_mcp_server(server_id: String) -> Result<Value, String> {
    let mut records = load_registry()?;
    let i = records
        .iter()
        .position(|item| item.id == server_id)
        .ok_or_else(|| "MCP server not found".to_string())?;
    if records[i].command.is_empty() {
        return Err("MCP server command is empty".into());
    }

    // Log the operation
    crate::storage::log_mcp_start(&server_id, &records[i].command);

    // Hold the process-manager lock across the whole check-then-spawn so two
    // concurrent starts can't both pass the liveness check and spawn duplicate,
    // orphaned processes (TOCTOU).
    {
        let mut pm = PROCESS_MANAGER.lock().map_err(|e| e.to_string())?;
        if let Some(child) = pm.get_mut(&server_id) {
            // Check if still alive
            if child.try_wait().map_err(|e| e.to_string())?.is_none() {
                records[i].running = true;
                records[i].pid = Some(child.id());
                save_registry(&records)?;
                return serde_json::to_value(records[i].clone()).map_err(|e| e.to_string());
            } else {
                // Process died, remove from manager
                pm.remove(&server_id);
            }
        }

        // Check legacy PID (for processes started before Child-handle tracking)
        if let Some(pid) = records[i].pid {
            if is_alive(pid) {
                records[i].running = true;
                save_registry(&records)?;
                return serde_json::to_value(records[i].clone()).map_err(|e| e.to_string());
            }
        }

        // Spawn while still holding the lock, then record the handle atomically.
        let child = spawn_server(&records[i], true)?;
        let pid = child.id();
        pm.insert(server_id.clone(), child);
        records[i].pid = Some(pid);
        records[i].running = true;
    }
    save_registry(&records)?;

    serde_json::to_value(records[i].clone()).map_err(|e| e.to_string())
}
pub fn stop_mcp_server(server_id: String) -> Result<Value, String> {
    crate::storage::log_mcp_stop(&server_id);

    let mut records = load_registry()?;
    let i = records
        .iter()
        .position(|item| item.id == server_id)
        .ok_or_else(|| "MCP server not found".to_string())?;

    // Try to kill via Child handle first
    {
        let mut pm = PROCESS_MANAGER.lock().map_err(|e| e.to_string())?;
        if let Some(mut child) = pm.remove(&server_id) {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    // Fallback: kill via PID (for legacy processes)
    if let Some(pid) = records[i].pid {
        stop_pid(pid)?;
    }

    records[i].pid = None;
    records[i].running = false;
    save_registry(&records)?;
    serde_json::to_value(records[i].clone()).map_err(|e| e.to_string())
}
pub fn get_mcp_logs(server_id: String) -> Result<String, String> {
    let records = load_registry()?;
    let record = records
        .iter()
        .find(|item| item.id == server_id)
        .ok_or_else(|| "MCP server not found".to_string())?;
    Ok(fs::read_to_string(&record.log_path).unwrap_or_default())
}

/// Cleanup all running MCP servers (called on app exit)
pub fn cleanup_all_servers() {
    if let Ok(mut pm) = PROCESS_MANAGER.lock() {
        for (id, mut child) in pm.drain() {
            // Try graceful kill first
            if let Err(e) = child.kill() {
                eprintln!("Failed to kill MCP server {}: {}", id, e);
            }

            // Spawn timeout watcher for force kill
            let pid: u32 = child.id();
            if pid > 1 {
                // pid > 1 to avoid killing init/system processes
                thread::spawn(move || {
                    thread::sleep(Duration::from_secs(5));
                    // Force kill if still alive after 5 seconds
                    if is_alive(pid) {
                        #[cfg(unix)]
                        {
                            let _ = Command::new("kill")
                                .args(["-9", &pid.to_string()])
                                .status();
                        }
                        #[cfg(windows)]
                        {
                            let _ = Command::new("taskkill")
                                .args(["/PID", &pid.to_string(), "/F"])
                                .status();
                        }
                    }
                });
            }

            // Non-blocking wait with timeout
            let start = std::time::Instant::now();
            while start.elapsed() < Duration::from_secs(2) {
                if child.try_wait().ok().flatten().is_some() {
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}
