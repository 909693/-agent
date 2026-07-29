//! 封面生图:调用用户配置的生图模型(OpenAI 兼容),拿到图片字节。
//! 支持两种接口形态:
//! - images: POST /v1/images/generations,返回 data[0].b64_json 或 data[0].url
//! - chat:   POST /v1/chat/completions,从返回文本里提取图片 url 或 data:base64
//! 拿到 bytes 后由 tomato::upload_cover 传到番茄换 thumb_uri。

use serde_json::{json, Value};
use std::time::Duration;

struct ImageConfig {
    base_url: String,
    api_key: String,
    model: String,
    size: String,
    api_type: String, // "images" | "chat"
    proxy_url: Option<String>,
}

fn load_config() -> Result<ImageConfig, String> {
    let cfg = crate::storage::load_image_config()?
        .ok_or("尚未配置封面生图模型:请在系统设置的「封面生图」里填写 API 地址、Key 与模型")?;
    let base_url = cfg["baseUrl"].as_str().unwrap_or("").trim().trim_end_matches('/').to_string();
    let api_key = cfg["apiKey"].as_str().unwrap_or("").trim().to_string();
    let model = cfg["model"].as_str().unwrap_or("").trim().to_string();
    let size = {
        let s = cfg["size"].as_str().unwrap_or("").trim();
        if s.is_empty() { "1024x1024".to_string() } else { s.to_string() }
    };
    let api_type = {
        let t = cfg["apiType"].as_str().unwrap_or("").trim();
        if t.is_empty() { "images".to_string() } else { t.to_string() }
    };
    if base_url.is_empty() || api_key.is_empty() || model.is_empty() {
        return Err("封面生图配置不完整:需要 API 地址、API Key 和模型名".into());
    }
    let proxy_url = cfg["proxyUrl"]
        .as_str()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    Ok(ImageConfig { base_url, api_key, model, size, api_type, proxy_url })
}

/// 把 base(可能是站点根 / 带 /v1)与 OpenAI 相对路径拼成完整 URL。
fn build_url(base: &str, path: &str) -> String {
    let b = base.trim_end_matches('/');
    if b.ends_with(path) {
        b.to_string()
    } else if b.ends_with("/v1") {
        format!("{}{}", b, path.trim_start_matches("/v1"))
    } else {
        format!("{}{}", b, path)
    }
}

/// 本项目 reqwest 以 default-features = false 构建,不含 system-proxy feature,
/// 环境变量代理必须手动接上(顺序与 llm/client.rs 一致:配置显式代理 > 环境变量)。
fn resolve_proxy(explicit: Option<&str>) -> Option<String> {
    if let Some(p) = explicit {
        return Some(p.to_string());
    }
    ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"]
        .iter()
        .find_map(|k| std::env::var(k).ok().map(|v| v.trim().to_string()))
        .filter(|v| !v.is_empty())
}

fn http_client(explicit_proxy: Option<&str>) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(180));
    if let Some(proxy_url) = resolve_proxy(explicit_proxy) {
        let proxy = reqwest::Proxy::all(&proxy_url)
            .map_err(|e| format!("封面生图代理地址无效({}): {}", proxy_url, e))?;
        builder = builder.proxy(proxy);
    }
    builder
        .build()
        .map_err(|e| format!("构造 HTTP 客户端失败: {}", e))
}

/// reqwest 错误的 Display 不含底层原因(dns/连接被拒/超时统统只显示
/// "error sending request"),把 source 链展开拼进错误信息。
fn err_chain(e: &dyn std::error::Error) -> String {
    let mut s = e.to_string();
    let mut cur = e.source();
    while let Some(inner) = cur {
        s.push_str(" ← ");
        s.push_str(&inner.to_string());
        cur = inner.source();
    }
    s
}

/// 生成一张封面,返回 (图片字节, content_type)。
pub async fn generate_image_bytes(prompt: &str) -> Result<(Vec<u8>, String), String> {
    let cfg = load_config()?;
    let client = http_client(cfg.proxy_url.as_deref())?;
    let content = if cfg.api_type == "chat" {
        gen_via_chat(&client, &cfg, prompt).await?
    } else {
        gen_via_images(&client, &cfg, prompt).await?
    };
    // content 可能是 data:image;base64,... 或 http(s) 图片 url,统一转成字节
    resolve_to_bytes(&client, &content).await
}

/// images 接口:/v1/images/generations
async fn gen_via_images(client: &reqwest::Client, cfg: &ImageConfig, prompt: &str) -> Result<String, String> {
    let url = build_url(&cfg.base_url, "/v1/images/generations");
    let body = json!({
        "model": cfg.model,
        "prompt": prompt,
        "n": 1,
        "size": cfg.size,
    });
    let res = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", cfg.api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求生图接口失败: {}", err_chain(&e)))?;
    let status = res.status();
    let text = res.text().await.map_err(|e| format!("读取生图响应失败: {}", e))?;
    if !status.is_success() {
        return Err(format!("生图接口报错(HTTP {}): {}", status, text.chars().take(300).collect::<String>()));
    }
    let json: Value = serde_json::from_str(&text)
        .map_err(|_| format!("生图返回非 JSON: {}", text.chars().take(200).collect::<String>()))?;
    let first = &json["data"][0];
    if let Some(b64) = first["b64_json"].as_str() {
        if !b64.is_empty() {
            return Ok(format!("data:image/png;base64,{}", b64));
        }
    }
    if let Some(u) = first["url"].as_str() {
        if !u.is_empty() {
            return Ok(u.to_string());
        }
    }
    Err(format!("生图返回里没有 b64_json 或 url: {}", text.chars().take(200).collect::<String>()))
}

/// chat 接口:/v1/chat/completions,从回复文本里挖图
async fn gen_via_chat(client: &reqwest::Client, cfg: &ImageConfig, prompt: &str) -> Result<String, String> {
    let url = build_url(&cfg.base_url, "/v1/chat/completions");
    let body = json!({
        "model": cfg.model,
        "messages": [{ "role": "user", "content": prompt }],
        "stream": false,
    });
    let res = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", cfg.api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求生图接口失败: {}", err_chain(&e)))?;
    let status = res.status();
    let text = res.text().await.map_err(|e| format!("读取生图响应失败: {}", e))?;
    if !status.is_success() {
        return Err(format!("生图接口报错(HTTP {}): {}", status, text.chars().take(300).collect::<String>()));
    }
    let json: Value = serde_json::from_str(&text)
        .map_err(|_| format!("生图返回非 JSON: {}", text.chars().take(200).collect::<String>()))?;
    let msg = &json["choices"][0]["message"];
    // 有的实现把图放 message.images[].image_url.url
    if let Some(u) = msg["images"][0]["image_url"]["url"].as_str() {
        if !u.is_empty() {
            return Ok(u.to_string());
        }
    }
    let content = msg["content"].as_str().unwrap_or("");
    if let Some(found) = extract_image_ref(content) {
        return Ok(found);
    }
    Err(format!("chat 生图回复里没找到图片(url 或 base64):\n{}", content.chars().take(200).collect::<String>()))
}

/// 从一段文本里提取图片引用:优先 data:image base64,其次 http(s) 图片链接。
fn extract_image_ref(text: &str) -> Option<String> {
    // data:image/xxx;base64,....
    if let Some(pos) = text.find("data:image/") {
        let rest = &text[pos..];
        let end = rest.find(|c: char| c == ')' || c == '"' || c == '\'' || c.is_whitespace())
            .unwrap_or(rest.len());
        return Some(rest[..end].to_string());
    }
    // http(s)://... 到分隔符为止,再校验像图片链接
    if let Some(pos) = text.find("http") {
        let rest = &text[pos..];
        let end = rest.find(|c: char| c == ')' || c == '"' || c == '\'' || c == ' ' || c == '\n')
            .unwrap_or(rest.len());
        let u = &rest[..end];
        if u.starts_with("http") {
            return Some(u.to_string());
        }
    }
    None
}

/// 把 data URI 或 http 图片链接落成字节。
async fn resolve_to_bytes(client: &reqwest::Client, reference: &str) -> Result<(Vec<u8>, String), String> {
    if let Some(rest) = reference.strip_prefix("data:") {
        // data:image/png;base64,XXXX
        let comma = rest.find(',').ok_or("data URI 缺少逗号分隔")?;
        let meta = &rest[..comma];
        let data = &rest[comma + 1..];
        let content_type = meta.split(';').next().unwrap_or("image/png").to_string();
        let bytes = b64_decode(data)?;
        if bytes.is_empty() {
            return Err("base64 图片解码后为空".into());
        }
        return Ok((bytes, content_type));
    }
    // 当作 http 图片链接下载
    let res = client.get(reference).send().await.map_err(|e| format!("下载生成的图片失败: {}", err_chain(&e)))?;
    if !res.status().is_success() {
        return Err(format!("下载图片失败(HTTP {})", res.status()));
    }
    let content_type = res
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or(s).to_string())
        .filter(|s| s.starts_with("image/"))
        .unwrap_or_else(|| {
            if reference.contains(".png") { "image/png".to_string() } else { "image/jpeg".to_string() }
        });
    let bytes = res.bytes().await.map_err(|e| format!("读取图片字节失败: {}", e))?.to_vec();
    if bytes.is_empty() {
        return Err("下载到的图片为空".into());
    }
    Ok((bytes, content_type))
}

/// 把图片字节 + content_type 拼成可直接喂给 <img src> 的 data URL。
/// 预览走本地 data:,绕开番茄 CDN 的防盗链(校验 Referer/签名),避免破图。
pub fn to_data_url(bytes: &[u8], content_type: &str) -> String {
    let ct = if content_type.starts_with("image/") { content_type } else { "image/png" };
    format!("data:{};base64,{}", ct, b64_encode(bytes))
}

/// 标准 base64 编码(带 '=' 填充)。
fn b64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 { TABLE[((n >> 6) & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[(n & 63) as usize] as char } else { '=' });
    }
    out
}

/// 标准 + URL-safe base64 解码,忽略换行/空白与非表内字符。
fn b64_decode(s: &str) -> Result<Vec<u8>, String> {
    let mut rev = [255u8; 256];
    let table = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for (i, &c) in table.iter().enumerate() {
        rev[c as usize] = i as u8;
    }
    rev[b'-' as usize] = 62; // url-safe
    rev[b'_' as usize] = 63;
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut buf: u32 = 0;
    let mut bits = 0u32;
    for &c in s.as_bytes() {
        let v = rev[c as usize];
        if v == 255 {
            continue; // '=' / 换行 / 空白 / 其它
        }
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Ok(out)
}
