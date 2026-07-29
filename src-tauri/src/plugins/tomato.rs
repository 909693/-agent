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

/// 列出番茄作品分类(MCP 返回含 JSON 代码块的文本,由前端解析成下拉选项)。
pub fn list_categories() -> Result<String, String> {
    call_tool("list_categories", json!({}), Duration::from_secs(60))
}

/// 在番茄创建一本新书。平台约束:书名 ≤15 字、简介 ≥50 字、封面 thumb_uri 必填
/// (空封面会被服务端以 -2「参数有误」拒绝)。建成后 MCP 默认将其设为当前小说。
pub fn create_book(
    book_name: &str,
    abstract_text: &str,
    thumb_uri: &str,
    gender: Option<String>,
    category: Option<String>,
    protagonist: Vec<String>,
) -> Result<String, String> {
    let name = book_name.trim();
    if name.is_empty() {
        return Err("书名不能为空".into());
    }
    let name_len = name.chars().count();
    if name_len > 15 {
        return Err(format!("书名不能超过 15 字(当前 {} 字)", name_len));
    }
    let abstract_text = abstract_text.trim();
    let abstract_len = abstract_text.chars().count();
    if abstract_len < 50 {
        return Err(format!("简介至少 50 字(当前 {} 字)", abstract_len));
    }
    let thumb_uri = thumb_uri.trim();
    if thumb_uri.is_empty() {
        return Err("封面 thumb_uri 必填:空封面会被番茄服务端拒绝。可在番茄后台上传封面后抓 upload_pic 返回的 uri,或复用已有书的 thumb_uri".into());
    }

    let mut args = json!({
        "book_name": name,
        "abstract": abstract_text,
        "thumb_uri": thumb_uri,
    });
    if let Some(g) = gender.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        args["gender"] = json!(g);
    }
    if let Some(c) = category.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        args["category"] = json!(c);
    }
    let roles: Vec<String> = protagonist
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if !roles.is_empty() {
        args["protagonist"] = json!(roles);
    }
    call_tool("create_book", args, Duration::from_secs(120))
}

/// 去掉正文开头自带的章节标题行(如「第二章 灵潮初涌」「# 第2章 xxx」)。
/// 发布时标题已单独传给番茄,正文再带一行标题就会在读者端显示双标题。
/// 判定从严(宁可漏删不误删),须同时满足:
/// - 第一段非空行(去掉 Markdown「#」前缀后)以「第」开头
/// - 「第」后 8 字内出现「章/回/节」,且中间全是数字或中文数字
/// - 整行不超过 40 字,且不含句末标点(。！？；…——含这些更像叙述句)
fn strip_leading_chapter_title(text: &str) -> &str {
    let trimmed = text.trim_start();
    // 全文只有一行时不动它:剥掉就没有正文了
    let Some((first_line, rest)) = trimmed.split_once('\n') else {
        return text;
    };
    let line = first_line.trim().trim_start_matches('#').trim();
    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() || chars[0] != '第' || chars.len() > 40 {
        return text;
    }
    if chars.iter().any(|c| "。！？；…—".contains(*c)) {
        return text;
    }
    let numerals = "0123456789零〇一二三四五六七八九十百千两";
    let mut matched = false;
    for (i, c) in chars.iter().enumerate().skip(1).take(8) {
        if matches!(c, '章' | '回' | '节') {
            matched = i > 1; // 「第章」不算,序号至少一位
            break;
        }
        if !numerals.contains(*c) {
            break;
        }
    }
    if matched {
        rest.trim_start()
    } else {
        text
    }
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
    // 正文开头自带标题行的,剥掉,避免番茄端双标题
    let text = strip_leading_chapter_title(&text);
    if text.trim().is_empty() {
        return Err("章节内容只有一行标题,没有正文,无法发布".into());
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

/// 把图片字节上传到番茄图床,返回 (pic_uri, pic_url)。
/// pic_uri 即建书所需的 thumb_uri。接口经实测:multipart 字段名为 `file`,
/// 走 /api/author/data/upload_pic_v2/v0/,鉴权同发布(Cookie + CSRF)。
/// 直连番茄(不经 MCP):图片字节大,base64 过 JSON-RPC 不划算,且鉴权本就在本地。
pub async fn upload_cover(image_bytes: Vec<u8>, content_type: String) -> Result<(String, String), String> {
    const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/148.0.0.0 Safari/537.36";
    let cfg = crate::storage::load_tomato_config()?
        .ok_or("尚未配置番茄鉴权:请先在发布/建书弹窗里填写 Cookie 与 CSRF")?;
    let cookie = cfg["cookie"].as_str().unwrap_or("").trim().to_string();
    let csrf = cfg["csrfToken"].as_str().unwrap_or("").trim().to_string();
    if cookie.is_empty() || csrf.is_empty() {
        return Err("缺少番茄鉴权(Cookie / X-Secsdk-Csrf-Token),无法上传封面".into());
    }

    let ext = if content_type.contains("png") { "png" } else { "jpg" };
    let part = reqwest::multipart::Part::bytes(image_bytes)
        .file_name(format!("cover.{}", ext))
        .mime_str(&content_type)
        .map_err(|e| format!("构造上传表单失败: {}", e))?;
    let form = reqwest::multipart::Form::new().part("file", part);

    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| format!("构造 HTTP 客户端失败: {}", e))?;

    let res = client
        .post("https://fanqienovel.com/api/author/data/upload_pic_v2/v0/?aid=2503&app_name=muye_novel")
        .header("Cookie", cookie)
        .header("User-Agent", UA)
        .header("Accept", "application/json, text/plain, */*")
        .header("Origin", "https://fanqienovel.com")
        .header("Referer", "https://fanqienovel.com/main/writer/")
        .header("X-Secsdk-Csrf-Token", csrf)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("上传封面到番茄失败: {}", e))?;

    let status = res.status();
    let text = res.text().await.map_err(|e| format!("读取番茄响应失败: {}", e))?;
    let json: Value = serde_json::from_str(&text)
        .map_err(|_| format!("番茄返回非 JSON(HTTP {}): {}", status, text.chars().take(160).collect::<String>()))?;
    let code = json["code"].as_i64().unwrap_or(-1);
    if code != 0 {
        let msg = json["message"].as_str().or_else(|| json["msg"].as_str()).unwrap_or("未知错误");
        let hint = if code == -3 || msg.contains("登录") || msg.to_lowercase().contains("csrf") {
            "(鉴权可能失效,请在发布配置里更新 Cookie / CSRF)"
        } else {
            ""
        };
        return Err(format!("番茄上传封面失败(code={}): {}{}", code, msg, hint));
    }
    let pic_uri = json["data"]["pic_uri"].as_str().unwrap_or("").to_string();
    let pic_url = json["data"]["pic_url"].as_str().unwrap_or("").to_string();
    if pic_uri.is_empty() {
        return Err(format!("番茄上传成功但未返回 pic_uri: {}", text.chars().take(160).collect::<String>()));
    }
    Ok((pic_uri, pic_url))
}

#[cfg(test)]
mod tests {
    use super::strip_leading_chapter_title;

    #[test]
    fn strips_common_title_lines() {
        // 中文数字 / 阿拉伯数字 / Markdown 前缀 / 回目
        for head in ["第二章 灵潮初涌", "第2章 灵潮初涌", "# 第1024章 决战", "第三回 风雪山神庙"] {
            let text = format!("{}\n\n  正文第一段。", head);
            assert_eq!(strip_leading_chapter_title(&text), "正文第一段。", "case: {}", head);
        }
    }

    #[test]
    fn keeps_narrative_first_lines() {
        // 叙述句开头带「第」/ 含句末标点 / 超长行 / 非章节词,都不能误删
        for text in [
            "第二天清晨,叶辰醒来。\n正文继续。",
            "第三章的内容他早已烂熟于心。\n正文继续。",
            "叶辰站起身。\n第二段。",
        ] {
            assert!(strip_leading_chapter_title(text).starts_with(text.split('\n').next().unwrap().trim()));
        }
    }

    #[test]
    fn keeps_single_line_text() {
        assert_eq!(strip_leading_chapter_title("第二章 灵潮初涌"), "第二章 灵潮初涌");
    }
}
