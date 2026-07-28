//! 番茄小说发布：通过用户本地的 tomato-writer-mcp(MCP stdio server)调用
//! 番茄作家后台接口。与 plugins::mcp 的进程托管不同,这里是真正的 MCP 客户端:
//! 每次调用临时拉起 MCP 进程,完成 initialize 握手后 tools/call,取回结果即退出,
//! 不常驻。鉴权(Cookie / CSRF)从 storage 的番茄配置读取,以环境变量注入 MCP。

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

struct TomatoConfig {
    node_path: String,
    script: String,
    cookie: String,
    csrf_token: String,
}

fn load_config() -> Result<TomatoConfig, String> {
    let cfg = crate::storage::load_tomato_config()?
        .ok_or("尚未配置番茄发布：请在发布对话框中填写 MCP 脚本路径与鉴权信息")?;
    let node_path = cfg["nodePath"].as_str().unwrap_or("").trim().to_string();
    let node_path = if node_path.is_empty() { "node".to_string() } else { node_path };
    let script = cfg["script"].as_str().unwrap_or("").trim().to_string();
    if script.is_empty() {
        return Err("未配置 MCP 脚本路径(tomato-writer-mcp 的 dist/index.js)".into());
    }
    if !Path::new(&script).is_absolute() || !Path::new(&script).is_file() {
        return Err(format!("MCP 脚本不存在或不是绝对路径: {}", script));
    }
    let cookie = cfg["cookie"].as_str().unwrap_or("").trim().to_string();
    let csrf_token = cfg["csrfToken"].as_str().unwrap_or("").trim().to_string();
    if cookie.is_empty() || csrf_token.is_empty() {
        return Err("缺少鉴权：请在发布配置中填写番茄作家后台的 Cookie 与 X-Secsdk-Csrf-Token".into());
    }
    Ok(TomatoConfig { node_path, script, cookie, csrf_token })
}

/// 解析可执行文件:含路径分隔符则原样使用;否则在 PATH 及常见 Homebrew
/// 目录中查找(GUI 方式启动时 PATH 往往不含 /opt/homebrew/bin)。
fn resolve_command(cmd: &str) -> String {
    if cmd.contains('/') {
        return cmd.to_string();
    }
    let mut dirs: Vec<PathBuf> = std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .map(PathBuf::from)
        .collect();
    dirs.push(PathBuf::from("/opt/homebrew/bin"));
    dirs.push(PathBuf::from("/usr/local/bin"));
    for dir in dirs {
        let candidate = dir.join(cmd);
        if candidate.is_file() {
            return candidate.to_string_lossy().to_string();
        }
    }
    cmd.to_string()
}

fn send_message(stdin: &mut ChildStdin, msg: &Value) -> Result<(), String> {
    let line = serde_json::to_string(msg).map_err(|e| e.to_string())?;
    stdin
        .write_all(line.as_bytes())
        .and_then(|_| stdin.write_all(b"\n"))
        .and_then(|_| stdin.flush())
        .map_err(|e| format!("向 MCP 写入失败(进程可能已退出): {}", e))
}

/// 从 stdout 行流中等待指定 id 的 JSON-RPC 响应,忽略通知与无法解析的行。
fn wait_response(rx: &Receiver<String>, id: i64, timeout: Duration) -> Result<Value, String> {
    let deadline = Instant::now() + timeout;
    loop {
        let remain = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| "等待 MCP 响应超时".to_string())?;
        let line = rx
            .recv_timeout(remain)
            .map_err(|_| "MCP 无响应或进程已退出".to_string())?;
        let msg: Value = match serde_json::from_str(line.trim()) {
            Ok(v) => v,
            Err(_) => continue, // 非协议输出,跳过
        };
        if msg.get("id").and_then(Value::as_i64) == Some(id) {
            return Ok(msg);
        }
    }
}

fn drive_mcp(child: &mut Child, tool: &str, args: Value, timeout: Duration) -> Result<String, String> {
    let mut stdin = child.stdin.take().ok_or("无法获取 MCP stdin")?;
    let stdout = child.stdout.take().ok_or("无法获取 MCP stdout")?;

    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(l) => {
                    if tx.send(l).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    send_message(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "retl", "version": "0.1.0" }
            }
        }),
    )?;
    let init = wait_response(&rx, 1, Duration::from_secs(20))?;
    if let Some(err) = init.get("error") {
        return Err(format!(
            "MCP 初始化失败: {}",
            err.get("message").and_then(Value::as_str).unwrap_or("未知错误")
        ));
    }
    send_message(&mut stdin, &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))?;

    send_message(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": { "name": tool, "arguments": args }
        }),
    )?;
    let resp = wait_response(&rx, 2, timeout)?;
    if let Some(err) = resp.get("error") {
        return Err(format!(
            "MCP 调用失败: {}",
            err.get("message").and_then(Value::as_str).unwrap_or("未知错误")
        ));
    }
    let result = resp.get("result").ok_or("MCP 响应缺少 result")?;
    let text = result["content"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|c| c["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    if result["isError"].as_bool().unwrap_or(false) {
        return Err(if text.is_empty() { "MCP 工具执行失败".to_string() } else { text });
    }
    if text.is_empty() {
        return Err("MCP 返回了空结果".into());
    }
    Ok(text)
}

/// 拉起 MCP 进程调用一个工具,结束后回收进程。失败时附带 stderr 摘要。
fn call_tool(tool: &str, args: Value, timeout: Duration) -> Result<String, String> {
    let cfg = load_config()?;
    let node = resolve_command(&cfg.node_path);
    let mut child = Command::new(&node)
        .arg(&cfg.script)
        .env("TOMATO_COOKIE", &cfg.cookie)
        .env("TOMATO_CSRF_TOKEN", &cfg.csrf_token)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动番茄 MCP 失败({} {}): {}", node, cfg.script, e))?;

    let stderr = child.stderr.take();
    let stderr_handle = thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut s) = stderr {
            let _ = s.read_to_string(&mut buf);
        }
        buf
    });

    let result = drive_mcp(&mut child, tool, args, timeout);
    let _ = child.kill();
    let _ = child.wait();

    result.map_err(|e| {
        let err_out = stderr_handle.join().unwrap_or_default();
        // 过滤启动横幅,只留真正的报错内容
        let noise: Vec<&str> = err_out
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.contains("已启动"))
            .collect();
        if noise.is_empty() {
            e
        } else {
            let excerpt: String = noise.join("\n").chars().take(600).collect();
            format!("{}\nMCP stderr: {}", e, excerpt)
        }
    })
}

/// 列出账号下所有小说(MCP 返回可读文本,含 book_id,由前端解析展示)。
pub fn list_novels() -> Result<String, String> {
    call_tool("list_novels", json!({}), Duration::from_secs(60))
}

/// 发布(或 dry_run 预览)一章:正文取自项目存储的章节文件。
pub fn publish_chapter(
    project_id: &str,
    chapter_number: u32,
    book_id: Option<String>,
    title: &str,
    publish_time: Option<String>,
    use_ai: bool,
    dry_run: bool,
) -> Result<String, String> {
    if chapter_number == 0 || chapter_number > 9999 {
        return Err("章节号必须在 1-9999 范围内".into());
    }
    let title = title.trim();
    if title.is_empty() {
        return Err("章节标题不能为空".into());
    }
    let chapter_file = format!("chapter_{:03}.json", chapter_number);
    let data = crate::storage::load_json(project_id, &chapter_file)?
        .ok_or("章节尚未写作,没有可发布的内容")?;
    let text = data["text"].as_str().unwrap_or("").trim().to_string();
    if text.is_empty() {
        return Err("章节内容为空,无法发布".into());
    }

    let mut args = json!({
        "title": title,
        "content": text,
        "use_ai": use_ai,
        "dry_run": dry_run,
    });
    if let Some(b) = book_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        args["book_id"] = json!(b);
    }
    if let Some(t) = publish_time.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        args["publish_time"] = json!(t);
    }
    // 真正提交要经番茄后台建草稿 + 发布两步网络请求,给足超时
    call_tool("publish_chapter", args, Duration::from_secs(120))
}
