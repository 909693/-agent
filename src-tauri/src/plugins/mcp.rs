use super::types::McpServerRecord;
use super::util::{
    extensions_root, git_clone_or_pull, mcp_registry_path, read_json_or_default, read_log_excerpt,
    remove_dir_if_exists, repo_id, repo_install_path, repo_name, write_json,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};
use uuid::Uuid;

fn load_registry() -> Result<Vec<McpServerRecord>, String> {
    read_json_or_default(&mcp_registry_path())
}
fn save_registry(records: &[McpServerRecord]) -> Result<(), String> {
    write_json(&mcp_registry_path(), records)
}
fn log_path_for(id: &str) -> PathBuf {
    extensions_root().join("logs").join(format!("mcp-{id}.log"))
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
fn read_prefill(
    path: &Path,
    fallback: &str,
) -> Result<
    (
        String,
        String,
        Vec<String>,
        BTreeMap<String, String>,
        String,
    ),
    String,
> {
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
    Ok((
        fallback.to_string(),
        String::new(),
        Vec::new(),
        BTreeMap::new(),
        path.to_string_lossy().to_string(),
    ))
}
fn resolved_command(record: &McpServerRecord) -> String {
    let cwd = if record.cwd.is_empty() {
        &record.install_path
    } else {
        &record.cwd
    };
    if Path::new(&record.command).is_absolute() || !record.command.contains('/') {
        return record.command.clone();
    }
    PathBuf::from(cwd)
        .join(&record.command)
        .to_string_lossy()
        .to_string()
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
    let mut cmd = Command::new(resolved_command(record));
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
    cmd.stdout(Stdio::from(log))
        .stderr(Stdio::from(err))
        .spawn()
        .map_err(|e| e.to_string())
}
fn is_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
fn stop_pid(pid: u32) -> Result<(), String> {
    let _ = Command::new("kill")
        .arg(pid.to_string())
        .status()
        .map_err(|e| e.to_string())?;
    thread::sleep(Duration::from_millis(300));
    if is_alive(pid) {
        let _ = Command::new("kill")
            .args(["-9", &pid.to_string()])
            .status()
            .map_err(|e| e.to_string())?;
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
    if record.log_path.is_empty() {
        record.log_path = existing
            .as_ref()
            .map(|v| v.log_path.clone())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| log_path_for(&record.id).to_string_lossy().to_string());
    }
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
    let mut records = load_registry()?;
    let i = records
        .iter()
        .position(|item| item.id == server_id)
        .ok_or_else(|| "MCP server not found".to_string())?;
    let mut child = spawn_server(&records[i], true)?;
    thread::sleep(Duration::from_millis(1200));
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
    if let Some(pid) = records[i].pid {
        if is_alive(pid) {
            records[i].running = true;
            save_registry(&records)?;
            return serde_json::to_value(records[i].clone()).map_err(|e| e.to_string());
        }
    }
    let child = spawn_server(&records[i], true)?;
    records[i].pid = Some(child.id());
    records[i].running = true;
    save_registry(&records)?;
    serde_json::to_value(records[i].clone()).map_err(|e| e.to_string())
}
pub fn stop_mcp_server(server_id: String) -> Result<Value, String> {
    let mut records = load_registry()?;
    let i = records
        .iter()
        .position(|item| item.id == server_id)
        .ok_or_else(|| "MCP server not found".to_string())?;
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
