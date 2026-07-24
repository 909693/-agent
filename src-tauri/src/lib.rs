mod engine;
mod llm;
mod models;
mod plugins;
mod storage;

use chrono::Utc;
use llm::client::{LlmClient, LlmConfig};
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::Emitter;
use uuid::Uuid;
use storage::{
    log_export_project, log_skill_install, log_skill_remove,
};

static BATCH_RUNNING: AtomicBool = AtomicBool::new(false);
static BATCH_CANCEL: AtomicBool = AtomicBool::new(false);

const CHUNKED_PLOT_THRESHOLD: u32 = 60;

#[derive(Clone, Serialize)]
struct PlotProgress {
    phase: String,
    current_act: u32,
    total_acts: u32,
    message: String,
}

/// Truncate string to at most `max` **characters** (not bytes).
/// Returns a valid UTF-8 substring.
fn truncate_chars(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((byte_idx, _)) => &s[..byte_idx],
        None => s, // string has fewer than max chars
    }
}

/// Escape HTML special characters to prevent XSS in exported HTML
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Safe byte-level truncation that respects char boundaries.
/// Used when we want to limit byte size (e.g., for API payload limits).
#[allow(dead_code)]
fn truncate_bytes(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes { return s; }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) { end -= 1; }
    &s[..end]
}

/// Sanitize an ID to prevent path traversal. Only allows alphanumeric, dash, underscore.
#[allow(dead_code)]
fn sanitize_id(id: &str) -> Result<&str, String> {
    if id.is_empty() {
        return Err("ID 不能为空".into());
    }
    if id.contains("..") || id.contains('/') || id.contains('\\') || id.contains('\0') {
        return Err(format!("无效 ID: {}", id));
    }
    Ok(id)
}

/// Validate base_url to prevent SSRF attacks.
/// Blocks internal/private IPs, file:// scheme, and non-HTTP(S) protocols.
fn validate_base_url(url: &str) -> Result<(), String> {
    if url.is_empty() {
        return Ok(()); // Empty means use default provider URL
    }

    // Must start with https:// or http://
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("Base URL 必须以 https:// 或 http:// 开头".into());
    }

    // Parse to extract host
    let parsed = url::Url::parse(url)
        .map_err(|_| "无效的 URL 格式".to_string())?;

    let host = parsed.host_str()
        .ok_or("URL 缺少主机名")?;

    // Block localhost and loopback
    if host == "localhost" || host == "127.0.0.1" || host == "::1" || host == "0.0.0.0" {
        return Err("不允许访问本地地址".into());
    }

    // Block private IP ranges
    if host.starts_with("10.") || host.starts_with("192.168.") {
        return Err("不允许访问内网地址".into());
    }

    // Check 172.16.0.0/12 (172.16.x.x - 172.31.x.x)
    if host.starts_with("172.") {
        if let Some(second) = host.split('.').nth(1) {
            if let Ok(n) = second.parse::<u8>() {
                if (16..=31).contains(&n) {
                    return Err("不允许访问内网地址".into());
                }
            }
        }
    }

    // Block metadata endpoints (cloud SSRF)
    if host == "169.254.169.254" || host == "metadata.google.internal" {
        return Err("不允许访问云元数据服务".into());
    }

    Ok(())
}

/// Validate data directory path to prevent writing to sensitive system locations.
fn validate_data_dir(dir: &str) -> Result<(), String> {
    if dir.is_empty() {
        return Err("目录路径不能为空".into());
    }

    let path = std::path::Path::new(dir);

    // Must be absolute
    if !path.is_absolute() {
        return Err("数据目录必须是绝对路径".into());
    }

    // Block sensitive system directories
    let blocked_prefixes = [
        "/etc", "/bin", "/sbin", "/usr/bin", "/usr/sbin",
        "/System", "/Library/System", "/var/root",
        "/private/etc", "/private/var/root",
    ];
    #[cfg(windows)]
    let blocked_prefixes_win = [
        "C:\\Windows", "C:\\Program Files", "C:\\Program Files (x86)",
        "C:\\ProgramData\\Microsoft",
    ];
    for prefix in &blocked_prefixes {
        if dir.starts_with(prefix) {
            return Err(format!("不允许使用系统目录: {}", prefix));
        }
    }
    #[cfg(windows)]
    {
        let dir_upper = dir.to_uppercase();
        for prefix in &blocked_prefixes_win {
            if dir_upper.starts_with(&prefix.to_uppercase()) {
                return Err(format!("不允许使用系统目录: {}", prefix));
            }
        }
    }

    // Block path traversal
    if dir.contains("..") || dir.contains('\0') {
        return Err("路径包含非法字符".into());
    }

    Ok(())
}

fn build_constraints_text(constraints: Option<&Value>) -> String {
    let Some(c) = constraints else { return String::new(); };
    let mode = c["mode"].as_str().unwrap_or("strict");
    let mut sections: Vec<String> = Vec::new();

    if let Some(skills) = c["skills"].as_array() {
        if !skills.is_empty() {
            let mut text = String::from("## 必须遵循的 Skills 规则\n");
            for skill in skills.iter().take(3) {
                let name = skill["name"].as_str().unwrap_or("未命名 Skill");
                let content = skill["content"].as_str().unwrap_or("");
                text.push_str(&format!("\n### Skill: {}\n{}\n", name, truncate_chars(content, 2000)));
            }
            sections.push(text);
        }
    }

    if let Some(prompts) = c["prompts"].as_array() {
        // Genre authoring guide (category "genre") gets its own section so it is
        // always included and doesn't consume the 3-slot user-prompt budget below.
        if let Some(g) = prompts.iter().find(|p| p["category"].as_str() == Some("genre")) {
            let content = g["content"].as_str().unwrap_or("");
            if !content.is_empty() {
                sections.push(format!("## 类型创作指引\n{}", truncate_chars(content, 1200)));
            }
        }
        let user_prompts: Vec<&Value> = prompts.iter()
            .filter(|p| p["category"].as_str() != Some("genre"))
            .collect();
        if !user_prompts.is_empty() {
            let mut text = String::from("## 必须应用的提示词\n");
            // 6 = 自动匹配的风格提示词（最多 3 条）+ 手动勾选的审校/自定义（最多 3 条）
            for prompt in user_prompts.iter().take(6) {
                let title = prompt["title"].as_str().unwrap_or("未命名提示词");
                let category = prompt["category"].as_str().unwrap_or("未分类");
                let content = prompt["content"].as_str().unwrap_or("");
                text.push_str(&format!("\n### 提示词: {}（{}）\n{}\n", title, category, truncate_chars(content, 800)));
            }
            sections.push(text);
        }
    }

    if sections.is_empty() {
        String::new()
    } else {
        let header = if mode == "strict" {
            "# 创作硬约束\n以下 Skills 与提示词是强制规则，生成内容必须严格遵守，不得偏离。\n\n"
        } else {
            "# 创作参考约束\n以下 Skills 与提示词是优先参考，请尽量遵循。\n\n"
        };
        format!("{}{}", header, sections.join("\n\n"))
    }
}

// ===== Shared Helpers =====

/// Find chapter outline from plot.json by chapter number
pub fn find_chapter_outline(project_id: &str, chapter_number: u32) -> Result<String, String> {
    let plot = storage::load_json(project_id, "plot.json")?.unwrap_or(json!({}));
    if let Some(acts) = plot["acts"].as_array() {
        for act in acts {
            if let Some(chapters) = act["chapters"].as_array() {
                for ch in chapters {
                    if ch["number"].as_u64() == Some(chapter_number as u64) {
                        return Ok(format!(
                            "第{}章：{}\n{}",
                            chapter_number,
                            ch["title"].as_str().unwrap_or(""),
                            ch["summary"].as_str().unwrap_or("")
                        ));
                    }
                }
            }
        }
    }
    Err(format!("Chapter {} not found in plot outline", chapter_number))
}

/// Build rich RAG context string for prompt injection
/// Uses smart windowing (first 3 + last 10 chapters), character states, foreshadowing
pub fn build_rich_context_string(project_id: &str, chapter_number: u32) -> Result<String, String> {
    let world = storage::load_json(project_id, "world.json")?.unwrap_or(json!({}));
    let chars = storage::load_json(project_id, "characters.json")?.unwrap_or(json!({}));
    let summaries = storage::load_json(project_id, "chapter_summaries.json")?.unwrap_or(json!({}));

    let mut prev_summaries: Vec<String> = Vec::new();
    let mut character_states: Vec<String> = Vec::new();
    let mut active_foreshadowing: Vec<String> = Vec::new();
    let mut resolved_foreshadowing: Vec<String> = Vec::new();

    for ch_num in 1..chapter_number {
        let key = ch_num.to_string();
        if let Some(s) = summaries.get(&key) {
            let summary_text = s["summary"].as_str().unwrap_or("");
            if !summary_text.is_empty() {
                prev_summaries.push(format!("第{}章：{}", ch_num, summary_text));
            }
            if let Some(changes) = s["character_changes"].as_array() {
                for c in changes {
                    let name = c["name"].as_str().unwrap_or("");
                    let change = c["change"].as_str().unwrap_or("");
                    if !name.is_empty() {
                        character_states.push(format!("第{}章 {}：{}", ch_num, name, change));
                    }
                }
            }
            if let Some(planted) = s["foreshadowing_planted"].as_array() {
                for f in planted {
                    if let Some(t) = f.as_str() {
                        active_foreshadowing.push(format!("第{}章埋设：{}", ch_num, t));
                    }
                }
            }
            if let Some(resolved) = s["foreshadowing_resolved"].as_array() {
                for f in resolved {
                    if let Some(t) = f.as_str() {
                        resolved_foreshadowing.push(t.to_string());
                    }
                }
            }
        }
    }

    // Remove resolved from active
    active_foreshadowing.retain(|f| {
        // Skip empty resolved entries: `contains("")` is always true and would
        // wipe every pending foreshadowing if the model emits an empty string.
        !resolved_foreshadowing.iter().any(|r| !r.is_empty() && f.contains(r))
    });

    // Fallback: if no summaries exist, read raw chapter text for context
    if prev_summaries.is_empty() && chapter_number > 1 {
        let start = if chapter_number > 3 { chapter_number - 3 } else { 1 };
        for ch_num in start..chapter_number {
            let file = format!("chapter_{:03}.json", ch_num);
            if let Ok(Some(ch)) = storage::load_json(project_id, &file) {
                if let Some(text) = ch["text"].as_str() {
                    if !text.is_empty() {
                        let chars_count = text.chars().count();
                        let tail = if chars_count > 800 {
                            let skip = chars_count - 800;
                            &text[text.char_indices().nth(skip).map(|(i, _)| i).unwrap_or(0)..]
                        } else {
                            text
                        };
                        prev_summaries.push(format!("第{}章（末尾）：...{}", ch_num, tail));
                    }
                }
            }
        }
    }

    // Last chapter end_state
    let last_end_state = if chapter_number > 1 {
        summaries.get((chapter_number - 1).to_string())
            .and_then(|s| s["end_state"].as_str())
            .unwrap_or("")
    } else { "" };

    // Smart windowing: first 3 + last 10
    let total = prev_summaries.len();
    let kept_summaries: Vec<&str> = if total <= 13 {
        prev_summaries.iter().map(|s| s.as_str()).collect()
    } else {
        let mut kept: Vec<&str> = prev_summaries[..3].iter().map(|s| s.as_str()).collect();
        kept.push("...");
        kept.extend(prev_summaries[total - 10..].iter().map(|s| s.as_str()));
        kept
    };

    // Recent character states (last 20)
    let states_len = character_states.len();
    let kept_states = if states_len <= 20 {
        &character_states[..]
    } else {
        &character_states[states_len - 20..]
    };

    // Build world + character brief
    let world_brief = truncate_chars(world["overview"].as_str().unwrap_or(""), 500);
    let era = world["era"].as_str().unwrap_or("");
    let char_names: Vec<String> = chars["characters"].as_array()
        .map(|a| a.iter().take(6).map(|c| {
            format!("{}（{}）", c["name"].as_str().unwrap_or(""), c["role"].as_str().unwrap_or(""))
        }).collect())
        .unwrap_or_default();

    // Assemble context string
    let mut parts: Vec<String> = Vec::new();

    parts.push(format!("## 世界观\n时代：{}\n概要：{}", era, world_brief));
    if !char_names.is_empty() {
        parts.push(format!("## 主要角色\n{}", char_names.join("、")));
    }
    if !kept_summaries.is_empty() {
        parts.push(format!("## 前文回顾\n{}", kept_summaries.join("\n")));
    }
    if !last_end_state.is_empty() {
        parts.push(format!("## 上一章结尾\n{}", last_end_state));
    }
    if !kept_states.is_empty() {
        parts.push(format!("## 角色状态变化\n{}", kept_states.join("\n")));
    }
    if !active_foreshadowing.is_empty() {
        parts.push(format!("## 未回收伏笔\n{}", active_foreshadowing.join("\n")));
    }

    let full = parts.join("\n\n");
    Ok(truncate_chars(&full, 3500).to_string())
}

/// Auto-summarize a chapter and save to chapter_summaries.json
async fn auto_summarize_and_save(
    client: &LlmClient,
    project_id: &str,
    chapter_number: u32,
) -> Result<Value, String> {
    let chapter_file = format!("chapter_{:03}.json", chapter_number);
    let chapter = storage::load_json(project_id, &chapter_file)?
        .ok_or("章节不存在")?;
    let chapter_text = chapter["text"].as_str().unwrap_or("");
    if chapter_text.is_empty() {
        return Err("章节内容为空，无法生成摘要".into());
    }
    let chapter_outline = find_chapter_outline(project_id, chapter_number).unwrap_or_default();
    let summary = engine::summarize_chapter(client, chapter_number, chapter_text, &chapter_outline).await?;

    let summaries_file = "chapter_summaries.json";
    // Serialize the read-modify-write so concurrent summaries (batch + manual)
    // don't lose each other's updates to the shared aggregate file.
    {
        let lock = storage::project_lock(project_id);
        let _g = lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut summaries = storage::load_json(project_id, summaries_file)?.unwrap_or(json!({}));
        summaries[chapter_number.to_string()] = summary.clone();
        storage::save_json(project_id, summaries_file, &summaries)?;
    }

    Ok(summary)
}

// --- Tauri Commands ---

#[tauri::command]
fn get_data_dir() -> String {
    storage::data_dir().to_string_lossy().to_string()
}

#[tauri::command]
fn save_llm_config(config: Value) -> Result<(), String> {
    storage::save_llm_config(&config)
}

#[tauri::command]
fn get_llm_config() -> Result<Value, String> {
    match storage::load_llm_config()? {
        Some(val) => Ok(val),
        None => Ok(json!(null)),
    }
}

#[tauri::command]
fn save_llm_profiles(profiles: Value) -> Result<(), String> {
    storage::save_llm_profiles(&profiles)
}

#[tauri::command]
fn get_llm_profiles() -> Result<Value, String> {
    match storage::load_llm_profiles()? {
        Some(val) => Ok(val),
        None => Ok(json!({})),
    }
}

#[tauri::command]
fn save_llm_providers(data: Value) -> Result<(), String> {
    storage::save_llm_providers(&data)
}

#[tauri::command]
fn get_llm_providers() -> Result<Value, String> {
    match storage::load_llm_providers()? {
        Some(val) => Ok(val),
        None => Ok(json!({ "activeId": "", "providers": [] })),
    }
}

#[tauri::command]
async fn test_llm(api_format: String, api_key: String, model: String, base_url: String, proxy_url: Option<String>, user_agent: Option<String>) -> Result<String, String> {
    // Validate base_url to prevent SSRF
    validate_base_url(&base_url)?;

    let ua = llm::client::parse_user_agent(&user_agent);
    let model_name = if model.is_empty() { "claude-sonnet-4-20250514".to_string() } else { model };

    match api_format.as_str() {
        "anthropic" => {
            let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));
            let body = serde_json::json!({
                "model": model_name,
                "max_tokens": 50,
                "messages": [{"role": "user", "content": "Say hi"}]
            });
            let body_str = serde_json::to_string_pretty(&body).unwrap_or_default();

            // Try x-api-key
            let mut builder1 = reqwest::Client::builder()
                .danger_accept_invalid_certs(false)
                .timeout(Duration::from_secs(60));
            if let Some(ref proxy) = proxy_url {
                if let Ok(p) = reqwest::Proxy::all(proxy) {
                    builder1 = builder1.proxy(p);
                }
            }
            if let Some(ref v) = ua {
                builder1 = builder1.user_agent(v.clone());
            }
            let r1 = builder1.build().map_err(|e| format!("Client build: {:?}", e))?
                .post(&url)
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .body(body_str.clone())
                .send().await;
            let (status1, text1) = match r1 {
                Ok(resp) => {
                    let s = resp.status().to_string();
                    let t = resp.text().await.unwrap_or_default();
                    (s, t)
                }
                Err(e) => ("FAILED".into(), format!("{:?}", e)),
            };

            // Try Bearer
            let mut builder2 = reqwest::Client::builder()
                .danger_accept_invalid_certs(false)
                .timeout(Duration::from_secs(60));
            if let Some(ref proxy) = proxy_url {
                if let Ok(p) = reqwest::Proxy::all(proxy) {
                    builder2 = builder2.proxy(p);
                }
            }
            if let Some(ref v) = ua {
                builder2 = builder2.user_agent(v.clone());
            }
            let r2 = builder2.build().map_err(|e| format!("Client build: {:?}", e))?
                .post(&url)
                .header("Authorization", format!("Bearer {}", api_key))
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .body(body_str.clone())
                .send().await;
            let (status2, text2) = match r2 {
                Ok(resp) => {
                    let s = resp.status().to_string();
                    let t = resp.text().await.unwrap_or_default();
                    (s, t)
                }
                Err(e) => ("FAILED".into(), format!("{:?}", e)),
            };

            Ok(format!("URL: {}\nBody: {}\n\n--- x-api-key ---\nStatus: {}\n{}\n\n--- Bearer ---\nStatus: {}\n{}",
                url, body_str, status1, truncate_chars(&text1, 500), status2, truncate_chars(&text2, 500)))
        }
        "openai-responses" => {
            let url = format!("{}/v1/responses", base_url.trim_end_matches('/'));
            let body = serde_json::json!({
                "model": model_name,
                "max_output_tokens": 50,
                "input": [{"role": "user", "content": "Say hi"}]
            });
            let mut builder = reqwest::Client::builder()
                .danger_accept_invalid_certs(false)
                .timeout(Duration::from_secs(60));
            if let Some(ref proxy) = proxy_url {
                if let Ok(p) = reqwest::Proxy::all(proxy) {
                    builder = builder.proxy(p);
                }
            }
            if let Some(ref v) = ua {
                builder = builder.user_agent(v.clone());
            }
            let resp = builder.build().map_err(|e| format!("Client build: {:?}", e))?
                .post(&url)
                .header("Authorization", format!("Bearer {}", api_key))
                .header("content-type", "application/json")
                .json(&body)
                .send().await.map_err(|e| format!("Request failed: {:?}", e))?;
            let status = resp.status().to_string();
            let text = resp.text().await.unwrap_or_default();
            Ok(format!("URL: {}\nStatus: {}\nResponse: {}", url, status, truncate_chars(&text, 500)))
        }
        _ => {
            let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
            let body = serde_json::json!({
                "model": model_name,
                "max_tokens": 50,
                "messages": [{"role": "user", "content": "Say hi"}]
            });
            let mut builder = reqwest::Client::builder()
                .danger_accept_invalid_certs(false)
                .timeout(Duration::from_secs(60));
            if let Some(ref proxy) = proxy_url {
                if let Ok(p) = reqwest::Proxy::all(proxy) {
                    builder = builder.proxy(p);
                }
            }
            if let Some(ref v) = ua {
                builder = builder.user_agent(v.clone());
            }
            let resp = builder.build().map_err(|e| format!("Client build: {:?}", e))?
                .post(&url)
                .header("Authorization", format!("Bearer {}", api_key))
                .header("content-type", "application/json")
                .json(&body)
                .send().await.map_err(|e| format!("Request failed: {:?}", e))?;
            let status = resp.status().to_string();
            let text = resp.text().await.unwrap_or_default();
            Ok(format!("URL: {}\nStatus: {}\nResponse: {}", url, status, truncate_chars(&text, 500)))
        }
    }
}

/// Extract model IDs from a provider's model-list response.
/// Handles OpenAI/Anthropic ({"data":[{"id":..}]}), Gemini ({"models":[{"name":"models/..."}]}),
/// and bare-array / string-array variants some proxies return.
fn extract_model_ids(data: &Value) -> Vec<String> {
    let empty = Vec::new();
    let items = data["data"]
        .as_array()
        .or_else(|| data["models"].as_array())
        .or_else(|| data.as_array())
        .unwrap_or(&empty);
    let mut ids: Vec<String> = Vec::new();
    for item in items {
        // Gemini lists embedding/TTS-only models too; keep only generateContent-capable
        // ones when the capability field is present.
        if let Some(methods) = item["supportedGenerationMethods"].as_array() {
            let can_generate = methods
                .iter()
                .any(|m| m.as_str().is_some_and(|s| s.contains("generateContent")));
            if !can_generate {
                continue;
            }
        }
        let id = item["id"]
            .as_str()
            .or_else(|| item["name"].as_str())
            .or_else(|| item.as_str());
        if let Some(id) = id {
            let id = id.strip_prefix("models/").unwrap_or(id);
            if !id.is_empty() {
                ids.push(id.to_string());
            }
        }
    }
    ids.sort();
    ids.dedup();
    ids
}

#[tauri::command]
async fn fetch_models(
    api_format: String,
    api_key: String,
    base_url: String,
    proxy_url: Option<String>,
    user_agent: Option<String>,
) -> Result<Vec<String>, String> {
    // Validate base_url to prevent SSRF
    validate_base_url(&base_url)?;

    let mut builder = reqwest::Client::builder()
        .danger_accept_invalid_certs(false)
        .timeout(Duration::from_secs(30));
    if let Some(ref proxy) = proxy_url {
        if let Ok(p) = reqwest::Proxy::all(proxy) {
            builder = builder.proxy(p);
        }
    }
    if let Some(v) = llm::client::parse_user_agent(&user_agent) {
        builder = builder.user_agent(v);
    }
    let client = builder.build().map_err(|e| format!("Client build: {:?}", e))?;

    let base = |default: &str| -> String {
        if base_url.is_empty() {
            default.to_string()
        } else {
            base_url.trim_end_matches('/').to_string()
        }
    };

    let (status, text) = match api_format.as_str() {
        "anthropic" => {
            let url = format!("{}/v1/models?limit=1000", base("https://api.anthropic.com"));
            let resp = client
                .get(&url)
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01")
                .send().await.map_err(|e| format!("请求失败: {}", e))?;
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            // 部分中转站只认 Bearer；认证类错误时换认证方式重试一次
            if matches!(status.as_u16(), 401 | 403 | 404) {
                let resp2 = client
                    .get(&url)
                    .header("Authorization", format!("Bearer {}", api_key))
                    .header("anthropic-version", "2023-06-01")
                    .send().await.map_err(|e| format!("请求失败: {}", e))?;
                let status2 = resp2.status();
                let text2 = resp2.text().await.unwrap_or_default();
                if status2.is_success() { (status2, text2) } else { (status, text) }
            } else {
                (status, text)
            }
        }
        "gemini" => {
            let url = format!("{}/v1beta/models?pageSize=1000", base("https://generativelanguage.googleapis.com"));
            let resp = client
                .get(&url)
                .header("x-goog-api-key", &api_key)
                .send().await.map_err(|e| format!("请求失败: {}", e))?;
            (resp.status(), resp.text().await.unwrap_or_default())
        }
        _ => {
            let url = format!("{}/v1/models", base("https://api.openai.com"));
            let resp = client
                .get(&url)
                .header("Authorization", format!("Bearer {}", api_key))
                .send().await.map_err(|e| format!("请求失败: {}", e))?;
            (resp.status(), resp.text().await.unwrap_or_default())
        }
    };

    if !status.is_success() {
        return Err(format!("拉取模型失败 ({}): {}", status, truncate_chars(&text, 300)));
    }
    let data: Value = serde_json::from_str(&text)
        .map_err(|_| format!("响应不是有效 JSON: {}", truncate_chars(&text, 300)))?;
    let models = extract_model_ids(&data);
    if models.is_empty() {
        return Err(format!("接口返回成功但未解析到模型，原始响应: {}", truncate_chars(&text, 300)));
    }
    Ok(models)
}

#[tauri::command]
fn set_data_dir(new_dir: String, migrate: bool) -> Result<String, String> {
    // Validate directory path
    validate_data_dir(&new_dir)?;

    let old_dir = storage::data_dir().to_string_lossy().to_string();
    if migrate && old_dir != new_dir {
        let count = storage::migrate_data(&old_dir, &new_dir)?;
        storage::set_custom_data_dir(&new_dir)?;
        Ok(format!("已迁移 {} 个项目到新目录", count))
    } else {
        storage::set_custom_data_dir(&new_dir)?;
        Ok("数据目录已更新".into())
    }
}

#[tauri::command]
async fn agent_chat(
    project_id: String,
    message: String,
    history: Vec<(String, String)>,
    api_format: String,
    api_key: String,
    model: String,
    base_url: String,
    proxy_url: Option<String>,
    user_agent: Option<String>,
) -> Result<Value, String> {
    // Input validation
    if message.len() > 10000 {
        return Err("消息过长（最大 10000 字符）".into());
    }
    if history.len() > 100 {
        return Err("对话历史过长（最多 100 条）".into());
    }

    let meta = storage::load_json(&project_id, "meta.json")?.unwrap_or(json!({}));
    let world = storage::load_json(&project_id, "world.json")?.unwrap_or(json!({}));
    let chars = storage::load_json(&project_id, "characters.json")?.unwrap_or(json!({}));
    let plot = storage::load_json(&project_id, "plot.json")?.unwrap_or(json!({}));
    let outline = storage::load_json(&project_id, "outline_source.json")?.unwrap_or(json!({}));

    let project_context = format!(
        "标题：{}\n类型：{}\n基调：{}\n前提：{}\n世界观摘要：{}\n角色数：{}\n章节数：{}\n已导入大纲：{}",
        meta["title"].as_str().unwrap_or("未设置"),
        meta["genre"].as_str().unwrap_or("未设置"),
        meta["tone"].as_str().unwrap_or("未设置"),
        truncate_chars(meta["premise"].as_str().unwrap_or("未设置"), 500),
        truncate_chars(world["overview"].as_str().unwrap_or("未生成"), 500),
        chars["characters"].as_array().map(|a| a.len()).unwrap_or(0),
        plot["acts"].as_array().map(|a| a.iter().flat_map(|act| act["chapters"].as_array()).flatten().count()).unwrap_or(0),
        if outline["text"].as_str().unwrap_or("").is_empty() { "未导入" } else { "已导入" },
    );

    let system = format!(
        r#"你是 RETL 小说创作智能体助手。你可以帮用户完成以下任务：

## 当前项目信息
{project_context}

## 你能做的事
1. **查询信息**：查看世界观、角色、情节大纲、章节内容
2. **生成框架**：生成世界观、角色、情节大纲（需要用户确认）
3. **写作辅助**：扩写章节、续写、补足字数、局部补写
4. **审校检查**：对章节进行全维度审校
5. **导出小说**：导出为 TXT
6. **回答问题**：关于当前小说的任何问题

## 回复格式
你的回复必须是**纯 JSON**，不要包含任何 markdown 代码块标记或其他文字：
{{
  "reply": "给用户看的自然语言回复",
  "action": null 或 {{
    "type": "动作类型",
    "params": {{}}
  }}
}}

## 示例
用户说"帮我写第一章"，你应该回复：
{{"reply": "好的，我来帮你扩写第一章，目标 3000 字。", "action": {{"type": "expand_chapter", "params": {{"chapter": 1, "target_words": 3000, "hint": ""}}}}}}

用户说"生成世界观"，你应该回复：
{{"reply": "好的，我来生成世界观。", "action": {{"type": "generate_world", "params": {{}}}}}}

## 动作类型列表
- "generate_world": 生成世界观
- "generate_characters": 生成角色
- "generate_plot": 生成情节大纲
- "expand_chapter": 扩写章节, params: {{"chapter": 章节号, "target_words": 目标字数, "hint": "补充要求"}}
- "continue_chapter": 续写章节, params: {{"chapter": 章节号, "target_words": 目标字数, "instruction": "续写指示"}}
- "review_chapter": 审校章节, params: {{"chapter": 章节号, "platform": "番茄/起点/纵横"}}
- "export": 导出小说
- "show_characters": 展示角色列表
- "show_world": 展示世界观
- "show_plot": 展示情节大纲
- "show_chapter": 展示某章内容, params: {{"chapter": 章节号}}
- null: 纯对话，无需执行动作

## 规则
- 当用户明确表达了要执行某个操作（如"帮我写第一章"、"扩写第二章"、"生成世界观"），你必须在 action 字段返回对应的动作，不要只回复文字而不带 action
- 前端会显示确认按钮让用户确认执行，所以你不需要自己追问"确认吗？"——直接返回 action 即可
- 只有当用户意图真的不明确（比如"帮我改改"但没说改哪里）时才追问
- 回复要简洁友好
- reply 字段必须是中文
- action 字段必须严格使用动作类型列表中的 type 值，不要自创类型"#
    );

    let client = make_client(&api_format, &api_key, &model, &base_url, proxy_url, user_agent);
    let mut msgs = history.clone();
    msgs.push(("user".to_string(), message));

    let raw = client.chat(&system, &msgs, 4096).await?;

    // Try parse JSON from LLM response, with multiple fallback strategies
    let trimmed = raw.trim();
    // Strategy 1: direct JSON parse
    if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
        if parsed.get("reply").is_some() {
            return Ok(parsed);
        }
    }
    // Strategy 2: extract from markdown code block
    if trimmed.contains("```") {
        let start = trimmed.find("```").map(|i| i + 3).unwrap_or(0).min(trimmed.len());
        let after = &trimmed[start..];
        let len = after.len();
        let content_start = after.find('\n').map(|i| i + 1).unwrap_or(len).min(len);
        let end = after[content_start..].rfind("```").map(|i| content_start + i).unwrap_or(len);
        let json_str = &after[content_start..end];
        if let Ok(parsed) = serde_json::from_str::<Value>(json_str.trim()) {
            if parsed.get("reply").is_some() {
                return Ok(parsed);
            }
        }
    }
    // Strategy 3: find outermost JSON object containing "reply"
    if let Some(first_brace) = trimmed.find('{') {
        let last_brace = trimmed.rfind('}').unwrap_or(trimmed.len());
        if last_brace > first_brace {
            let candidate = &trimmed[first_brace..=last_brace];
            if let Ok(parsed) = serde_json::from_str::<Value>(candidate) {
                if parsed.get("reply").is_some() {
                    return Ok(parsed);
                }
            }
        }
    }
    // Fallback: treat entire response as plain text reply
    Ok(json!({"reply": raw, "action": null}))
}

static AGENT_CANCEL: AtomicBool = AtomicBool::new(false);

fn is_prompt_too_long(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("prompt is too long")
        || lower.contains("context_length_exceeded")
        || lower.contains("prompt too long")
        || lower.contains("request too large")
        || lower.contains("maximum context length")
        || lower.contains("resource_exhausted")
        || lower.contains("token limit")
}

/// True for generation errors that retrying can't fix — auth failures, an
/// exhausted balance/quota, or an over-long prompt. Batch generation retries
/// everything else (network blips, rate limits, malformed JSON) until the
/// chapter succeeds, but these would just fail identically forever, so the batch
/// stops and surfaces the reason instead of spinning.
fn batch_error_is_fatal(err: &str) -> bool {
    if is_prompt_too_long(err) {
        return true;
    }
    let l = err.to_lowercase();
    [
        "401", "403", "unauthorized", "authentication",
        "invalid api key", "invalid_api_key", "incorrect api key",
        "insufficient", "quota exceeded", "余额", "欠费",
    ]
    .iter()
    .any(|k| l.contains(k))
}

#[tauri::command]
async fn cancel_agent_chat() -> Result<(), String> {
    AGENT_CANCEL.store(true, Ordering::Relaxed);
    Ok(())
}

/// Resolves as soon as `flag` flips to true (polled at ~80ms). Raced against
/// long-running awaits (streaming LLM calls, tool/chapter generation) so a
/// cancel request interrupts in-flight work promptly instead of only being
/// noticed between coarse steps — which left work running (looked like "取消没反应").
async fn wait_cancel(flag: &'static AtomicBool) {
    while !flag.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
}

/// Await `$fut`, but bail out of the whole command the instant cancellation is
/// requested. Dropping the raced future aborts its HTTP request, so no further
/// tokens are produced. On cancel, emit the cancel event and return.
macro_rules! agent_race {
    ($app:expr, $fut:expr) => {
        tokio::select! {
            biased;
            _ = wait_cancel(&AGENT_CANCEL) => {
                let _ = $app.emit("agent_event", llm::client::StreamEvent::Error { error: "已取消".into() });
                return Ok(());
            }
            r = $fut => r,
        }
    };
}

#[tauri::command]
async fn agent_chat_stream(
    app: tauri::AppHandle,
    project_id: String,
    message: String,
    history: Vec<serde_json::Value>,
    constraints: Option<Value>,
    api_format: String,
    api_key: String,
    model: String,
    base_url: String,
    proxy_url: Option<String>,
    user_agent: Option<String>,
) -> Result<(), String> {
    use llm::client::{AgentMsg, StreamEvent};
    use engine::tools;

    if message.len() > 10000 {
        return Err("消息过长（最大 10000 字符）".into());
    }

    AGENT_CANCEL.store(false, Ordering::Relaxed);

    let meta = storage::load_json(&project_id, "meta.json")?.unwrap_or(json!({}));
    let world = storage::load_json(&project_id, "world.json")?.unwrap_or(json!({}));
    let chars = storage::load_json(&project_id, "characters.json")?.unwrap_or(json!({}));
    let plot = storage::load_json(&project_id, "plot.json")?.unwrap_or(json!({}));

    let project_context = format!(
        "标题：{}\n类型：{}\n基调：{}\n前提：{}\n世界观摘要：{}\n角色数：{}\n章节数：{}",
        meta["title"].as_str().unwrap_or("未设置"),
        meta["genre"].as_str().unwrap_or("未设置"),
        meta["tone"].as_str().unwrap_or("未设置"),
        truncate_chars(meta["premise"].as_str().unwrap_or("未设置"), 500),
        truncate_chars(world["overview"].as_str().unwrap_or("未生成"), 500),
        chars["characters"].as_array().map(|a| a.len()).unwrap_or(0),
        plot["acts"].as_array().map(|a| a.iter().flat_map(|act| act["chapters"].as_array()).flatten().count()).unwrap_or(0),
    );

    let system = format!(
        r#"你是 RETL 小说创作智能体助手。你可以通过工具帮用户完成各种小说创作任务。

## 当前项目信息
{project_context}

## 使用工具的规则
- 当用户要求写/扩写/续写章节时，直接调用 expand_chapter 或 continue_chapter，不需要先查看大纲或前面章节——这些工具内部会自动获取大纲、前文上下文、角色状态等所有必要信息
- 重要：当用户要求连续写多章时（如"写第5章和第6章"），必须一章一章按顺序写，每次只调用一个 expand_chapter，等前一章完成后再写下一章。这样后面的章节才能获取到前面章节的上下文，保证情节连贯
- 当用户要求生成世界观/角色/大纲时，直接调用对应工具
- 当用户要求查看信息（如"角色有哪些"），调用查询工具获取数据后回答
- 你可以连续调用多个工具完成复杂任务（如"生成完整框架"可以依次调用 generate_world → generate_characters → generate_plot）
- 工具执行结果会返回给你，你需要根据结果生成友好的中文回复
- 用自然、简洁的中文回答用户
- 尽量减少不必要的工具调用，能一步完成的不要分多步"#
    );

    let client = std::sync::Arc::new(make_client(&api_format, &api_key, &model, &base_url, proxy_url, user_agent));
    let tool_defs = tools::get_tool_definitions();

    // Build conversation from history + new message
    let mut conversation: Vec<AgentMsg> = Vec::new();
    for msg_val in &history {
        let role = msg_val["role"].as_str().unwrap_or("user");
        let content = msg_val["content"].as_str().unwrap_or("");
        match role {
            "user" => conversation.push(AgentMsg::User { content: content.to_string() }),
            "assistant" => conversation.push(AgentMsg::Assistant {
                text: content.to_string(),
                tool_uses: Vec::new(),
            }),
            _ => {}
        }
    }
    conversation.push(AgentMsg::User { content: message });

    let max_iterations = 10;
    let constraints_text = build_constraints_text(constraints.as_ref());
    eprintln!("[Agent] constraints_text length: {} chars", constraints_text.chars().count());
    let max_context_tokens: usize = 20000;

    for iteration in 0..max_iterations {
        // Compact conversation if too long
        let send_msgs = engine::context::compact_conversation(&conversation, max_context_tokens);

        if AGENT_CANCEL.load(Ordering::Relaxed) {
            let _ = app.emit("agent_event", StreamEvent::Error { error: "已取消".into() });
            return Ok(());
        }

        eprintln!("[Agent] Iteration {}/{}", iteration + 1, max_iterations);

        let app_clone = app.clone();
        let response = agent_race!(app, client.chat_with_tools_stream(
            &system,
            &send_msgs,
            &tool_defs,
            4096,
            move |delta| {
                let _ = app_clone.emit("agent_event", StreamEvent::Token { delta: delta.to_string() });
            },
        ));

        let response = match response {
            Ok(r) => r,
            Err(e) => {
                if is_prompt_too_long(&e) {
                    eprintln!("[Agent] Prompt too long, attempting aggressive compaction...");
                    let compact_msgs = engine::context::aggressive_compact(&conversation, max_context_tokens);
                    let app_retry = app.clone();
                    let retry = agent_race!(app, client.chat_with_tools_stream(
                        &system, &compact_msgs, &tool_defs, 4096,
                        move |delta| {
                            let _ = app_retry.emit("agent_event", StreamEvent::Token { delta: delta.to_string() });
                        },
                    ));
                    match retry {
                        Ok(r) => r,
                        Err(e2) => {
                            let _ = app.emit("agent_event", StreamEvent::Error {
                                error: format!("上下文过长，压缩后仍然失败: {}", e2),
                            });
                            return Ok(());
                        }
                    }
                } else {
                    let _ = app.emit("agent_event", StreamEvent::Error { error: e });
                    return Ok(());
                }
            }
        };

        // Max output tokens recovery: auto-continue if truncated
        let mut final_response = response;
        if final_response.stop_reason.as_deref() == Some("max_tokens") && final_response.tool_uses.is_empty() {
            for cont in 0..3u32 {
                eprintln!("[Agent] Output truncated (continuation {}/3), requesting more...", cont + 1);
                conversation.push(AgentMsg::Assistant {
                    text: final_response.text.clone(),
                    tool_uses: Vec::new(),
                });
                conversation.push(AgentMsg::User {
                    content: "你的输出被截断了，请从断点继续。不要重复已输出的内容。".into(),
                });
                let cont_msgs = engine::context::compact_conversation(&conversation, max_context_tokens);
                let app_cont = app.clone();
                let cont_resp = agent_race!(app, client.chat_with_tools_stream(
                    &system, &cont_msgs, &tool_defs, 4096,
                    move |delta| {
                        let _ = app_cont.emit("agent_event", StreamEvent::Token { delta: delta.to_string() });
                    },
                ));
                conversation.pop(); // remove temp user msg
                conversation.pop(); // remove temp assistant msg
                match cont_resp {
                    Ok(r) => {
                        final_response.text.push_str(&r.text);
                        final_response.tool_uses = r.tool_uses;
                        final_response.stop_reason = r.stop_reason;
                        if final_response.stop_reason.as_deref() != Some("max_tokens") || !final_response.tool_uses.is_empty() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        }

        // Add assistant response to conversation
        conversation.push(AgentMsg::Assistant {
            text: final_response.text.clone(),
            tool_uses: final_response.tool_uses.clone(),
        });

        // If no tool calls, we're done
        if final_response.tool_uses.is_empty() {
            let _ = app.emit("agent_event", StreamEvent::Done { reply: final_response.text });
            return Ok(());
        }

        // Execute tools — read-only tools run concurrently, write tools run sequentially
        if AGENT_CANCEL.load(Ordering::Relaxed) {
            let _ = app.emit("agent_event", StreamEvent::Error { error: "已取消".into() });
            return Ok(());
        }

        let (read_only, write): (Vec<_>, Vec<_>) = final_response.tool_uses.iter()
            .partition(|tc| tools::is_tool_read_only(&tc.name));

        // Emit all ToolCall events upfront
        for tc in read_only.iter().chain(write.iter()) {
            let _ = app.emit("agent_event", StreamEvent::ToolCall {
                id: tc.id.clone(), name: tc.name.clone(), input: tc.input.clone(),
            });
        }

        // Run read-only tools concurrently
        if !read_only.is_empty() {
            let read_futures: Vec<_> = read_only.iter().map(|tc| {
                let name = tc.name.clone();
                let input = tc.input.clone();
                let id = tc.id.clone();
                let pid = project_id.clone();
                let ct = constraints_text.clone();
                let cl = client.clone();
                async move {
                    let result = tools::execute_tool(&name, &input, &pid, &cl, &ct).await;
                    (id, name, result)
                }
            }).collect();

            let results = agent_race!(app, futures_util::future::join_all(read_futures));
            for (id, name, result) in results {
                let (success, result_str) = match &result {
                    Ok(v) => (true, serde_json::to_string_pretty(v).unwrap_or_default()),
                    Err(e) => (false, format!("错误: {}", e)),
                };
                let display_result = if result_str.chars().count() > 500 {
                    format!("{}...", result_str.chars().take(500).collect::<String>())
                } else { result_str.clone() };
                let _ = app.emit("agent_event", StreamEvent::ToolResult {
                    name: name.clone(), success, result: display_result,
                });
                let llm_result = if result_str.chars().count() > 1000 {
                    format!("{}...(已截断)", result_str.chars().take(1000).collect::<String>())
                } else { result_str };
                conversation.push(AgentMsg::ToolResultMsg { tool_use_id: id, content: llm_result });
            }
        }

        // Run write tools sequentially
        for tool_call in &write {
            if AGENT_CANCEL.load(Ordering::Relaxed) {
                let _ = app.emit("agent_event", StreamEvent::Error { error: "已取消".into() });
                return Ok(());
            }
            let result = agent_race!(app, tools::execute_tool(
                &tool_call.name, &tool_call.input, &project_id, &client, &constraints_text,
            ));
            let (success, result_str) = match &result {
                Ok(v) => (true, serde_json::to_string_pretty(v).unwrap_or_default()),
                Err(e) => (false, format!("错误: {}", e)),
            };
            let display_result = if result_str.chars().count() > 500 {
                format!("{}...", result_str.chars().take(500).collect::<String>())
            } else { result_str.clone() };
            let _ = app.emit("agent_event", StreamEvent::ToolResult {
                name: tool_call.name.clone(), success, result: display_result,
            });
            let llm_result = if result_str.chars().count() > 1000 {
                format!("{}...(已截断)", result_str.chars().take(1000).collect::<String>())
            } else { result_str };
            conversation.push(AgentMsg::ToolResultMsg {
                tool_use_id: tool_call.id.clone(), content: llm_result,
            });
        }
        // Continue loop to let LLM process tool results
    }

    let _ = app.emit("agent_event", StreamEvent::Done {
        reply: "已达到最大迭代次数。".into(),
    });
    Ok(())
}

#[tauri::command]
async fn chat_with_ai(
    messages: Vec<(String, String)>,
    genre: String,
    constraints: Option<Value>,
    api_format: String,
    api_key: String,
    model: String,
    base_url: String,
    proxy_url: Option<String>,
    user_agent: Option<String>,
) -> Result<String, String> {
    // Input validation
    if messages.len() > 100 {
        return Err("对话历史过长（最多 100 条）".into());
    }
    if genre.len() > 50 {
        return Err("类型名称过长".into());
    }

    let client = make_client(&api_format, &api_key, &model, &base_url, proxy_url, user_agent);
    let constraints_raw = build_constraints_text(constraints.as_ref());
    let constraints_text = truncate_chars(&constraints_raw, 2000);
    let system = format!(
        r#"你是一位专业的小说策划师，正在帮助用户构思一部{}类型的小说。

{}

你的任务是通过对话逐步了解用户想写的故事，引导他们完善以下要素：
1. 核心设定和世界观
2. 主要角色（主角、反派、关键配角）
3. 核心冲突和故事主线
4. 基调和风格
5. 主题

对话规则：
- 每次只问 1-2 个问题，不要一次问太多
- 根据用户的回答自然地深入追问
- 如果用户的想法模糊，给出 2-3 个具体建议让他们选择
- 用轻松友好的语气，像朋友聊天一样
- 在整个对话过程中，必须遵循上面的创作约束
- 当你觉得信息已经足够构建故事框架时，告诉用户"我已经有足够的信息了，让我来帮你整理故事框架！"，然后在回复末尾加上标记 [FRAMEWORK_READY]

第一条消息应该热情地打招呼，然后问用户最想写什么样的故事。"#,
        genre, constraints_text
    );
    // Anthropic requires at least one message - add default if empty
    let msgs = if messages.is_empty() {
        vec![("user".to_string(), format!("我想写一部{}类型的小说，请开始引导我。", genre))]
    } else {
        messages
    };
    client.chat(&system, &msgs, 1024).await
}

#[tauri::command]
async fn extract_framework(
    messages: Vec<(String, String)>,
    genre: String,
    constraints: Option<Value>,
    api_format: String,
    api_key: String,
    model: String,
    base_url: String,
    proxy_url: Option<String>,
    user_agent: Option<String>,
) -> Result<Value, String> {
    let client = make_client(&api_format, &api_key, &model, &base_url, proxy_url, user_agent);
    let constraints_text = build_constraints_text(constraints.as_ref());
    let system = format!("你是一位专业的小说策划师。请根据之前的对话内容，提取并整理出完整的小说框架。输出严格的 JSON 格式。\n\n{}", constraints_text);
    let mut msgs = messages.clone();
    msgs.push((
        "user".to_string(),
        format!(
            r#"请根据我们之前的对话，整理出完整的小说框架，输出 JSON：
{{
  "title": "小说标题（根据内容起一个吸引人的标题）",
  "genre": "{}",
  "premise": "故事前提（一段完整的描述，200-300字）",
  "tone": "基调",
  "themes": ["主题1", "主题2"],
  "protagonist": "主角简介",
  "antagonist": "反派简介",
  "core_conflict": "核心冲突",
  "world_brief": "世界观简述"
}}"#,
            genre
        ),
    ));
    client
        .chat(&system, &msgs, 4096)
        .await
        .and_then(|text| parse_framework_json(&text))
}

fn parse_framework_json(text: &str) -> Result<Value, String> {
    let trimmed = text.trim();
    let json_str = if trimmed.contains("```") {
        let start = trimmed.find("```").map(|i| i + 3).unwrap_or(0).min(trimmed.len());
        let after_fence = &trimmed[start..];
        let len = after_fence.len();
        let content_start = after_fence.find('\n').map(|i| i + 1).unwrap_or(len).min(len);
        let end = after_fence[content_start..].rfind("```").map(|i| content_start + i).unwrap_or(len);
        &after_fence[content_start..end]
    } else if let Some(start) = trimmed.find('{') {
        let end = trimmed.rfind('}').unwrap_or(start);
        if end >= start {
            &trimmed[start..=end]
        } else {
            &trimmed[start..]
        }
    } else {
        trimmed
    };
    serde_json::from_str(json_str.trim())
        .map_err(|e| format!("JSON parse error: {e}\nRaw: {json_str}"))
}

#[tauri::command]
async fn create_project(
    title: String,
    genre: String,
    premise: String,
    tone: String,
    themes: Vec<String>,
    target_chapter_words: u32,
) -> Result<Value, String> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let meta = json!({
        "id": id,
        "title": title,
        "genre": genre,
        "premise": premise,
        "tone": tone,
        "themes": themes,
        "target_chapter_words": target_chapter_words,
        "language": "zh-CN",
        "status": "init",
        "created_at": now,
        "updated_at": now,
    });
    storage::save_json(&id, "meta.json", &meta)?;
    Ok(meta)
}

#[tauri::command]
async fn list_projects() -> Result<Vec<Value>, String> {
    storage::list_projects()
}

#[tauri::command]
async fn get_project(project_id: String) -> Result<Value, String> {
    storage::load_json(&project_id, "meta.json")?.ok_or_else(|| "Project not found".into())
}

#[tauri::command]
async fn save_outline_source(project_id: String, outline: Value) -> Result<Value, String> {
    validate_json_size(&outline, "大纲")?;
    storage::save_json(&project_id, "outline_source.json", &outline)?;
    Ok(outline)
}

#[tauri::command]
async fn get_outline_source(project_id: String) -> Result<Value, String> {
    storage::load_json(&project_id, "outline_source.json")?.ok_or_else(|| "Outline source not found".into())
}

#[tauri::command]
async fn delete_project(project_id: String) -> Result<(), String> {
    storage::delete_project(&project_id)
}

#[tauri::command]
async fn list_skills() -> Result<Value, String> {
    plugins::list_skills()
}

#[tauri::command]
async fn install_skill_repo(repo_url: String) -> Result<Value, String> {
    log_skill_install(&repo_url);
    plugins::install_skill_repo(repo_url)
}

#[tauri::command]
async fn update_skill_repo(skill_id: String) -> Result<Value, String> {
    plugins::update_skill_repo(skill_id)
}

#[tauri::command]
async fn toggle_skill_repo(skill_id: String, enabled: bool) -> Result<Value, String> {
    plugins::toggle_skill_repo(skill_id, enabled)
}

#[tauri::command]
async fn remove_skill_repo(skill_id: String) -> Result<(), String> {
    log_skill_remove(&skill_id);
    plugins::remove_skill_repo(skill_id)
}

#[tauri::command]
async fn get_skill_detail(skill_id: String) -> Result<Value, String> {
    plugins::get_skill_detail(skill_id)
}

#[tauri::command]
async fn read_skill_file(skill_id: String, relative_path: String) -> Result<String, String> {
    plugins::read_skill_file(skill_id, relative_path)
}

#[tauri::command]
async fn list_mcp_servers() -> Result<Value, String> {
    plugins::list_mcp_servers()
}

#[tauri::command]
async fn install_mcp_repo(repo_url: String) -> Result<Value, String> {
    plugins::install_mcp_repo(repo_url)
}

#[tauri::command]
async fn save_mcp_server(server: Value) -> Result<Value, String> {
    plugins::save_mcp_server(server)
}

#[tauri::command]
async fn delete_mcp_server(server_id: String) -> Result<(), String> {
    plugins::delete_mcp_server(server_id)
}

#[tauri::command]
async fn test_mcp_server(server_id: String) -> Result<Value, String> {
    plugins::test_mcp_server(server_id)
}

#[tauri::command]
async fn start_mcp_server(server_id: String) -> Result<Value, String> {
    plugins::start_mcp_server(server_id)
}

#[tauri::command]
async fn stop_mcp_server(server_id: String) -> Result<Value, String> {
    plugins::stop_mcp_server(server_id)
}

#[tauri::command]
async fn get_mcp_logs(server_id: String) -> Result<String, String> {
    plugins::get_mcp_logs(server_id)
}

fn make_client(api_format: &str, api_key: &str, model: &str, base_url: &str, proxy_url: Option<String>, user_agent: Option<String>) -> LlmClient {
    let default_model = match api_format {
        "anthropic" => "claude-sonnet-4-20250514",
        "openai" => "gpt-4o",
        "gemini" => "gemini-2.5-flash",
        _ => "gpt-4o",
    };
    LlmClient::new(LlmConfig {
        provider: api_format.to_string(),
        api_key: api_key.to_string(),
        model: if model.is_empty() {
            default_model.to_string()
        } else {
            model.to_string()
        },
        base_url: base_url.to_string(),
        proxy_url,
        user_agent,
        accept_invalid_certs: false, // Default to secure mode
    })
}

#[tauri::command]
async fn generate_world(
    project_id: String,
    constraints: Option<Value>,
    api_format: String,
    api_key: String,
    model: String,
    base_url: String,
    proxy_url: Option<String>,
    user_agent: Option<String>,
) -> Result<Value, String> {
    let meta = storage::load_json(&project_id, "meta.json")?.ok_or("Project not found")?;
    let outline_source = storage::load_json(&project_id, "outline_source.json")?.unwrap_or(json!({}));
    let client = make_client(&api_format, &api_key, &model, &base_url, proxy_url, user_agent);
    let constraints_text = build_constraints_text(constraints.as_ref());
    let premise = meta["premise"].as_str().unwrap_or("");
    let genre = meta["genre"].as_str().unwrap_or("");
    let tone = meta["tone"].as_str().unwrap_or("");
    let themes: Vec<String> = meta["themes"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let world = engine::generate_world(
        &client,
        &format!("{}\n\n{}\n\n## 已导入大纲\n{}", constraints_text, premise, truncate_chars(&serde_json::to_string_pretty(&outline_source).unwrap_or_default(), 3000)),
        genre,
        tone,
        &themes,
    )
    .await?;
    storage::save_json(&project_id, "world.json", &world)?;
    Ok(world)
}

#[tauri::command]
async fn get_world(project_id: String) -> Result<Value, String> {
    storage::load_json(&project_id, "world.json")?.ok_or_else(|| "World not generated yet".into())
}

/// Max allowed size for user-submitted JSON data (5MB)
const MAX_JSON_PAYLOAD_SIZE: usize = 5_000_000;

/// Validate JSON payload size to prevent memory exhaustion
fn validate_json_size(data: &Value, label: &str) -> Result<(), String> {
    let size = serde_json::to_string(data).map(|s| s.len()).unwrap_or(0);
    if size > MAX_JSON_PAYLOAD_SIZE {
        return Err(format!("{} 数据过大（最大 5MB）", label));
    }
    Ok(())
}

#[tauri::command]
async fn save_world_data(project_id: String, world: Value) -> Result<Value, String> {
    validate_json_size(&world, "世界观")?;
    storage::save_json(&project_id, "world.json", &world)?;
    Ok(world)
}

#[tauri::command]
async fn generate_characters(
    project_id: String,
    constraints: Option<Value>,
    api_format: String,
    api_key: String,
    model: String,
    base_url: String,
    proxy_url: Option<String>,
    user_agent: Option<String>,
) -> Result<Value, String> {
    let meta = storage::load_json(&project_id, "meta.json")?.ok_or("Project not found")?;
    let world = storage::load_json(&project_id, "world.json")?.ok_or("Generate world first")?;
    let outline_source = storage::load_json(&project_id, "outline_source.json")?.unwrap_or(json!({}));
    let client = make_client(&api_format, &api_key, &model, &base_url, proxy_url, user_agent);
    let constraints_text = build_constraints_text(constraints.as_ref());
    let world_summary = serde_json::to_string(&json!({
        "era": world["era"], "overview": world["overview"],
        "factions": world["factions"]
    }))
    .unwrap_or_default();
    let chars = engine::generate_characters(
        &client,
        &format!("{}\n\n{}\n\n## 已导入大纲\n{}", constraints_text, meta["premise"].as_str().unwrap_or(""), truncate_chars(&serde_json::to_string_pretty(&outline_source).unwrap_or_default(), 3000)),
        meta["genre"].as_str().unwrap_or(""),
        meta["tone"].as_str().unwrap_or(""),
        &world_summary,
    )
    .await?;
    storage::save_json(&project_id, "characters.json", &chars)?;
    Ok(chars)
}

#[tauri::command]
async fn get_characters(project_id: String) -> Result<Value, String> {
    storage::load_json(&project_id, "characters.json")?
        .ok_or_else(|| "Characters not generated yet".into())
}

#[tauri::command]
async fn save_characters_data(project_id: String, characters: Value) -> Result<Value, String> {
    validate_json_size(&characters, "角色")?;
    storage::save_json(&project_id, "characters.json", &characters)?;
    Ok(characters)
}

/// Generate one act's (or sub-chunk's) chapter details, retrying on BOTH request
/// failure and incomplete output — a "successful" response with missing chapters
/// used to be accepted silently, leaving whole acts empty in the outline (UI
/// showed e.g. ch.29 jumping straight to ch.59). Returns the chapters array.
async fn generate_act_chapters_with_retry(
    client: &LlmClient,
    act_number: u32,
    act_title: &str,
    act_theme: &str,
    act_key_events: &str,
    act_end_state: &str,
    chapter_start: u32,
    chapter_end: u32,
    story_context: &str,
    prev_act_summary: &str,
    next_act_summary: &str,
    plot_points_json: &str,
) -> Result<Vec<Value>, String> {
    let expected = (chapter_end - chapter_start + 1) as usize;
    let mut last_err = String::new();
    for attempt in 0..3u32 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(2 * attempt as u64)).await;
        }
        match engine::generate_act_chapters(
            client, act_number, act_title, act_theme, act_key_events, act_end_state,
            chapter_start, chapter_end, story_context, prev_act_summary, next_act_summary, plot_points_json,
        ).await {
            Ok(v) => {
                let mut chapters = v["chapters"].as_array().cloned().unwrap_or_default();
                if chapters.len() >= expected {
                    chapters.truncate(expected); // drop over-generation beyond the range
                    return Ok(chapters);
                }
                last_err = format!("模型只返回了 {}/{} 章", chapters.len(), expected);
                eprintln!("[act_chapters] 第 {} 幕(第{}-{}章)产出不完整({})，重试 {}/2",
                    act_number, chapter_start, chapter_end, last_err, attempt + 1);
            }
            Err(e) => {
                eprintln!("[act_chapters] 第 {} 幕(第{}-{}章)请求失败: {}，重试 {}/2",
                    act_number, chapter_start, chapter_end, e, attempt + 1);
                last_err = e;
            }
        }
    }
    Err(format!("第 {} 幕（第 {}-{} 章）生成失败（已重试 2 次）：{}", act_number, chapter_start, chapter_end, last_err))
}

async fn generate_plot_chunked(
    app: &tauri::AppHandle,
    client: &LlmClient,
    premise: &str,
    genre: &str,
    tone: &str,
    world_summary: &str,
    characters_summary: &str,
    target_chapters: u32,
) -> Result<Value, String> {
    let _ = app.emit("plot_progress", PlotProgress {
        phase: "skeleton".into(),
        current_act: 0,
        total_acts: 0,
        message: format!("正在生成 {} 章的故事骨架...", target_chapters),
    });

    let skeleton = engine::generate_plot_skeleton(
        client, premise, genre, tone, world_summary, characters_summary, target_chapters,
    ).await?;

    let acts = skeleton["acts"].as_array()
        .ok_or("骨架生成失败：未返回 acts 数组")?;
    if acts.is_empty() {
        return Err("骨架生成失败：acts 数组为空".into());
    }
    let total_acts_count = acts.len() as u32;
    eprintln!("[generate_plot_chunked] Phase 1 done: {} acts, target {} chapters", total_acts_count, target_chapters);

    // Compute exact chapter ranges in code: divide target_chapters by act count
    // Extra chapters (remainder) are distributed to the first N acts
    let base_chapters = target_chapters / total_acts_count;
    let remainder = target_chapters % total_acts_count;
    let plot_points_json = serde_json::to_string(&skeleton["plot_points"]).unwrap_or("[]".into());
    let story_context = format!(
        "前提：{}\n类型：{}\n基调：{}\n世界观：{}\n角色：{}",
        truncate_chars(premise, 500), genre, tone,
        truncate_chars(world_summary, 500), truncate_chars(characters_summary, 500)
    );

    let mut merged_acts: Vec<Value> = Vec::new();

    for (idx, act) in acts.iter().enumerate() {
        let act_num = act["number"].as_u64().unwrap_or((idx + 1) as u64) as u32;
        let act_title = act["title"].as_str().unwrap_or("");
        let act_theme = act["theme"].as_str().unwrap_or("");
        let act_key_events = serde_json::to_string(&act["key_events"]).unwrap_or("[]".into());
        let act_end_state = act["end_state"].as_str().unwrap_or("");

        // Compute chapter range from code: distribute chapters evenly
        // Earlier acts get +1 chapter if there's a remainder
        let ch_count = if (idx as u32) < remainder { base_chapters + 1 } else { base_chapters };
        let mut chapter_start = 1u32;
        for i in 0..idx {
            let extra = if (i as u32) < remainder { 1 } else { 0 };
            chapter_start += base_chapters + extra;
        }
        let chapter_end = chapter_start + ch_count - 1;

        let prev_summary = if idx > 0 {
            let prev = &acts[idx - 1];
            format!("前一幕：{}（{}）- 结局状态：{}",
                prev["title"].as_str().unwrap_or(""),
                prev["theme"].as_str().unwrap_or(""),
                prev["end_state"].as_str().unwrap_or(""))
        } else { String::new() };

        let next_summary = if idx + 1 < acts.len() {
            let next = &acts[idx + 1];
            format!("后一幕：{}（{}）",
                next["title"].as_str().unwrap_or(""),
                next["theme"].as_str().unwrap_or(""))
        } else { String::new() };

        let _ = app.emit("plot_progress", PlotProgress {
            phase: "act_details".into(),
            current_act: (idx + 1) as u32,
            total_acts: total_acts_count,
            message: format!("正在生成第 {} 幕章节详情（第 {}-{} 章）...", act_num, chapter_start, chapter_end),
        });

        // Sub-chunk large acts to fit within provider output limits (~16384 tokens max)
        // At ~600 tokens/chapter, each chunk fits ~22 chapters
        let act_chapters_count = chapter_end - chapter_start + 1;
        let chunk_size = 22;
        // A failed act ABORTS the whole outline generation (with a clear error)
        // instead of silently leaving the act empty — an outline with a hole in
        // the middle breaks batch generation and reads as data loss to the user.
        let all_chapters: Vec<Value> = if act_chapters_count <= chunk_size {
            // Small act: single call
            generate_act_chapters_with_retry(
                client, act_num, act_title, act_theme, &act_key_events, act_end_state,
                chapter_start, chapter_end, &story_context, &prev_summary, &next_summary, &plot_points_json,
            ).await?
        } else {
            // Large act: split into sub-chunks
            let mut chunks: Vec<Value> = Vec::new();
            let mut sub_start = chapter_start;
            let mut sub_idx = 0;
            while sub_start <= chapter_end {
                sub_idx += 1;
                let sub_end = (sub_start + chunk_size - 1).min(chapter_end);
                if sub_idx > 1 {
                    // Small delay between sub-chunks to avoid rate limiting
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
                let got = generate_act_chapters_with_retry(
                    client, act_num, act_title, act_theme, &act_key_events, act_end_state,
                    sub_start, sub_end, &story_context, &prev_summary, &next_summary, &plot_points_json,
                ).await?;
                eprintln!("[generate_plot_chunked] Act {} sub-chunk {} (ch {}-{}) got {} chapters",
                    act_num, sub_idx, sub_start, sub_end, got.len());
                chunks.extend(got);
                sub_start = sub_end + 1;
            }
            chunks
        };

        merged_acts.push(json!({
            "number": act_num,
            "title": act_title,
            "theme": act_theme,
            "chapters": all_chapters,
        }));
    }

    let _ = app.emit("plot_progress", PlotProgress {
        phase: "done".into(),
        current_act: total_acts_count,
        total_acts: total_acts_count,
        message: "大纲生成完成".into(),
    });

    let final_plot = json!({
        "acts": merged_acts,
        "plot_points": skeleton["plot_points"],
        "subplots": skeleton["subplots"],
    });

    eprintln!("[generate_plot_chunked] Done: {} acts merged", merged_acts.len());
    Ok(final_plot)
}

#[tauri::command]
async fn generate_plot(
    app: tauri::AppHandle,
    project_id: String,
    constraints: Option<Value>,
    target_chapters: Option<u32>,
    api_format: String,
    api_key: String,
    model: String,
    base_url: String,
    proxy_url: Option<String>,
    user_agent: Option<String>,
) -> Result<Value, String> {
    eprintln!("[generate_plot] proxy_url = {:?}", proxy_url);
    let meta = storage::load_json(&project_id, "meta.json")?.ok_or("Project not found")?;
    let world = storage::load_json(&project_id, "world.json")?.ok_or("Generate world first")?;
    let chars =
        storage::load_json(&project_id, "characters.json")?.ok_or("Generate characters first")?;
    let outline_source = storage::load_json(&project_id, "outline_source.json")?.unwrap_or(json!({}));
    let client = make_client(&api_format, &api_key, &model, &base_url, proxy_url.clone(), user_agent.clone());
    let constraints_text = build_constraints_text(constraints.as_ref());
    let world_summary = serde_json::to_string(&json!({
        "era": world["era"], "overview": world["overview"]
    }))
    .unwrap_or_default();
    let chars_summary = serde_json::to_string(&chars["characters"]).unwrap_or_default();

    let chapters = target_chapters.unwrap_or(50);
    let premise_text = format!(
        "{}\n\n{}\n\n## 已导入大纲\n{}",
        constraints_text,
        meta["premise"].as_str().unwrap_or(""),
        truncate_chars(&serde_json::to_string_pretty(&outline_source).unwrap_or_default(), 3000)
    );

    let plot = if chapters > CHUNKED_PLOT_THRESHOLD {
        generate_plot_chunked(
            &app, &client, &premise_text,
            meta["genre"].as_str().unwrap_or(""),
            meta["tone"].as_str().unwrap_or(""),
            &world_summary, &chars_summary, chapters,
        ).await?
    } else {
        engine::generate_plot(
            &client, &premise_text,
            meta["genre"].as_str().unwrap_or(""),
            meta["tone"].as_str().unwrap_or(""),
            &world_summary, &chars_summary, target_chapters,
        ).await?
    };

    // Don't persist an outline with missing chapters — a hole in the middle
    // breaks batch generation later and reads as data loss in the chapter list.
    let got_chapters = plot["acts"].as_array()
        .map(|a| a.iter().flat_map(|act| act["chapters"].as_array()).flatten().count())
        .unwrap_or(0);
    // Allow modest deviation for the single-shot path (model may restructure),
    // but an outline at <80% of target means whole sections went missing.
    let min_ok = (chapters as usize) * 8 / 10;
    if got_chapters < min_ok.max(1) {
        return Err(format!(
            "大纲不完整：目标 {} 章，实际只生成了 {} 章，已放弃保存。请重试（可能是 API 响应被截断）",
            chapters, got_chapters
        ));
    }

    storage::save_json(&project_id, "plot.json", &plot)?;
    Ok(plot)
}

#[tauri::command]
async fn get_plot(project_id: String) -> Result<Value, String> {
    storage::load_json(&project_id, "plot.json")?.ok_or_else(|| "Plot not generated yet".into())
}

#[tauri::command]
async fn save_plot_outline(project_id: String, plot: Value) -> Result<Value, String> {
    validate_json_size(&plot, "情节大纲")?;
    storage::save_json(&project_id, "plot.json", &plot)?;
    Ok(plot)
}

#[tauri::command]
async fn generate_timeline(
    project_id: String,
    api_format: String,
    api_key: String,
    model: String,
    base_url: String,
    proxy_url: Option<String>,
    user_agent: Option<String>,
) -> Result<Value, String> {
    let world = storage::load_json(&project_id, "world.json")?.ok_or("Generate world first")?;
    let plot = storage::load_json(&project_id, "plot.json")?.ok_or("Generate plot first")?;
    let client = make_client(&api_format, &api_key, &model, &base_url, proxy_url, user_agent);
    let plot_summary = serde_json::to_string(&plot["acts"]).unwrap_or_default();
    let world_summary = serde_json::to_string(&json!({
        "era": world["era"], "history": world["history"]
    }))
    .unwrap_or_default();
    let timeline = engine::generate_timeline(&client, &plot_summary, &world_summary).await?;
    storage::save_json(&project_id, "timeline.json", &timeline)?;
    Ok(timeline)
}

#[tauri::command]
async fn get_timeline(project_id: String) -> Result<Value, String> {
    storage::load_json(&project_id, "timeline.json")?
        .ok_or_else(|| "Timeline not generated yet".into())
}

#[tauri::command]
async fn expand_chapter(
    project_id: String,
    chapter_number: u32,
    user_content: String,
    target_words: u32,
    constraints: Option<Value>,
    api_format: String,
    api_key: String,
    model: String,
    base_url: String,
    proxy_url: Option<String>,
    user_agent: Option<String>,
) -> Result<Value, String> {
    // Input validation
    if chapter_number == 0 || chapter_number > 9999 {
        return Err("章节号必须在 1-9999 范围内".into());
    }
    if target_words == 0 || target_words > 50000 {
        return Err("目标字数必须在 1-50000 范围内".into());
    }
    if user_content.len() > 100000 {
        return Err("输入内容过长（最大 100000 字符）".into());
    }

    let chapter_outline = find_chapter_outline(&project_id, chapter_number)?;
    let mut context = build_rich_context_string(&project_id, chapter_number)?;
    // Inject style profile if available
    if let Ok(Some(style)) = storage::load_json(&project_id, "style_profile.json") {
        if let Some(summary) = style["summary"].as_str() {
            context.push_str(&format!("\n\n## 作者文风\n{}", summary));
        }
    }
    let constraints_text = build_constraints_text(constraints.as_ref());

    let client = make_client(&api_format, &api_key, &model, &base_url, proxy_url, user_agent);
    let result = engine::expand_chapter(
        &client,
        &format!("{}\n\n{}", truncate_chars(&constraints_text, 2000), chapter_outline),
        &user_content,
        target_words,
        &context,
    )
    .await?;
    // Snapshot before overwriting, under the project lock so a concurrent reorder
    // or save can't interleave and lose the update.
    let chapter_file = format!("chapter_{:03}.json", chapter_number);
    {
        let lock = storage::project_lock(&project_id);
        let _g = lock.lock().unwrap_or_else(|e| e.into_inner());
        if let Ok(Some(existing)) = storage::load_json(&project_id, &chapter_file) {
            let old_text = existing["text"].as_str().unwrap_or("");
            if !old_text.is_empty() {
                let _ = storage::save_snapshot(&project_id, chapter_number, old_text);
            }
        }
        storage::save_json(&project_id, &chapter_file, &result)?;
    }
    Ok(result)
}

#[tauri::command]
async fn continue_writing(
    project_id: String,
    chapter_number: u32,
    instruction: String,
    target_words: u32,
    constraints: Option<Value>,
    api_format: String,
    api_key: String,
    model: String,
    base_url: String,
    proxy_url: Option<String>,
    user_agent: Option<String>,
) -> Result<Value, String> {
    // Input validation
    if chapter_number == 0 || chapter_number > 9999 {
        return Err("章节号必须在 1-9999 范围内".into());
    }
    if target_words == 0 || target_words > 50000 {
        return Err("目标字数必须在 1-50000 范围内".into());
    }
    if instruction.len() > 10000 {
        return Err("指令过长（最大 10000 字符）".into());
    }

    let chapter_file = format!("chapter_{:03}.json", chapter_number);
    let existing = storage::load_json(&project_id, &chapter_file)?
        .ok_or("Chapter not found, use expand first")?;
    let existing_text = existing["text"].as_str().unwrap_or("");

    let mut context = build_rich_context_string(&project_id, chapter_number)?;
    if let Ok(Some(style)) = storage::load_json(&project_id, "style_profile.json") {
        if let Some(summary) = style["summary"].as_str() {
            context.push_str(&format!("\n\n## 作者文风\n{}", summary));
        }
    }
    let constraints_text = build_constraints_text(constraints.as_ref());
    let client = make_client(&api_format, &api_key, &model, &base_url, proxy_url, user_agent);
    // Truncate existing text to last ~2000 characters to avoid token overflow
    let existing_tail = {
        let chars_count = existing_text.chars().count();
        if chars_count > 2000 {
            let skip = chars_count - 2000;
            &existing_text[existing_text.char_indices().nth(skip).map(|(i, _)| i).unwrap_or(0)..]
        } else {
            existing_text
        }
    };
    let result = engine::continue_writing(
        &client,
        existing_tail,
        &format!("{}\n\n{}", truncate_chars(&constraints_text, 2000), instruction),
        target_words,
        &context,
    )
    .await?;
    // Persist under the project lock. Re-read first: if the chapter changed during
    // generation (a reorder or another save), abort rather than overwrite with a
    // continuation based on now-stale text. Snapshot so the append is recoverable.
    let lock = storage::project_lock(&project_id);
    let _g = lock.lock().unwrap_or_else(|e| e.into_inner());
    let current = storage::load_json(&project_id, &chapter_file)?
        .ok_or("Chapter not found")?;
    let current_text = current["text"].as_str().unwrap_or("");
    if current_text != existing_text {
        return Err("章节在生成期间被修改，续写已取消，请重试".into());
    }
    if !existing_text.is_empty() {
        let _ = storage::save_snapshot(&project_id, chapter_number, existing_text);
    }
    // Append to existing chapter
    let new_text = format!("{}\n\n{}", existing_text, result["text"].as_str().unwrap_or(""));
    let updated = json!({"text": new_text});
    storage::save_json(&project_id, &chapter_file, &updated)?;
    Ok(updated)
}

#[tauri::command]
async fn save_chapter(project_id: String, chapter_number: u32, text: String, snapshot: Option<bool>) -> Result<(), String> {
    // Input validation
    if chapter_number == 0 || chapter_number > 9999 {
        return Err("章节号必须在 1-9999 范围内".into());
    }
    if text.len() > 500000 {
        return Err("章节内容过长（最大 500000 字符）".into());
    }

    // Snapshot before overwriting, unless the caller opts out (auto-save does, to
    // avoid flooding snapshots every few seconds) and only when content changed.
    let chapter_file = format!("chapter_{:03}.json", chapter_number);
    // Serialize this chapter file's read-modify-write against concurrent writers
    // (e.g. a chapter reorder / batch) so updates aren't lost. No .await while held.
    let lock = storage::project_lock(&project_id);
    let _g = lock.lock().unwrap_or_else(|e| e.into_inner());
    if snapshot.unwrap_or(true) {
        if let Ok(Some(existing)) = storage::load_json(&project_id, &chapter_file) {
            let old_text = existing["text"].as_str().unwrap_or("");
            if !old_text.is_empty() && old_text != text {
                let _ = storage::save_snapshot(&project_id, chapter_number, old_text);
            }
        }
    }
    storage::save_json(&project_id, &chapter_file, &json!({"text": text}))
}

/// Swap two chapters' stored text and summaries. Used when the outline reorders
/// chapters (their numbers swap) so number-keyed text/summaries follow along and
/// don't desync from the outline entries.
#[tauri::command]
async fn swap_chapters(project_id: String, a: u32, b: u32) -> Result<(), String> {
    if a == 0 || b == 0 || a > 9999 || b > 9999 {
        return Err("章节号必须在 1-9999 范围内".into());
    }
    if a == b {
        return Ok(());
    }
    if BATCH_RUNNING.load(Ordering::SeqCst) {
        return Err("批量生成进行中，请先取消或等待完成后再重排章节".into());
    }
    // Serialize against concurrent writers to the same project's files.
    let lock = storage::project_lock(&project_id);
    let _g = lock.lock().unwrap_or_else(|e| e.into_inner());

    let fa = format!("chapter_{:03}.json", a);
    let fb = format!("chapter_{:03}.json", b);
    let va = storage::load_json(&project_id, &fa)?.unwrap_or_else(|| json!({"text": ""}));
    let vb = storage::load_json(&project_id, &fb)?.unwrap_or_else(|| json!({"text": ""}));
    // Snapshot both chapters before the destructive overwrites so a half-completed
    // swap (e.g. the second save fails) is recoverable. swap_snapshots below then
    // carries these backups along with the content.
    if let Some(t) = va["text"].as_str() { if !t.is_empty() { let _ = storage::save_snapshot(&project_id, a, t); } }
    if let Some(t) = vb["text"].as_str() { if !t.is_empty() { let _ = storage::save_snapshot(&project_id, b, t); } }
    storage::save_json(&project_id, &fa, &vb)?;
    storage::save_json(&project_id, &fb, &va)?;

    // Swap the matching summary entries so RAG/consistency stay aligned, fixing
    // each moved summary's internal "chapter" field to match its new key.
    let summaries_file = "chapter_summaries.json";
    if let Some(mut summaries) = storage::load_json(&project_id, summaries_file)? {
        if let Some(obj) = summaries.as_object_mut() {
            let ka = a.to_string();
            let kb = b.to_string();
            let sa = obj.get(&ka).cloned();
            let sb = obj.get(&kb).cloned();
            match sb {
                Some(mut v) => {
                    if let Some(o) = v.as_object_mut() { o.insert("chapter".into(), json!(a)); }
                    obj.insert(ka.clone(), v);
                }
                None => { obj.remove(&ka); }
            }
            match sa {
                Some(mut v) => {
                    if let Some(o) = v.as_object_mut() { o.insert("chapter".into(), json!(b)); }
                    obj.insert(kb.clone(), v);
                }
                None => { obj.remove(&kb); }
            }
            storage::save_json(&project_id, summaries_file, &summaries)?;
        }
    }

    // Snapshots are also keyed by chapter number — swap them so each chapter's
    // version history follows the reorder (otherwise "restore" would overwrite
    // the moved content with another chapter's old text).
    storage::swap_snapshots(&project_id, a, b)?;
    Ok(())
}

#[tauri::command]
async fn get_chapter(project_id: String, chapter_number: u32) -> Result<Value, String> {
    // Input validation
    if chapter_number == 0 || chapter_number > 9999 {
        return Err("章节号必须在 1-9999 范围内".into());
    }

    let chapter_file = format!("chapter_{:03}.json", chapter_number);
    storage::load_json(&project_id, &chapter_file)?.ok_or_else(|| "Chapter not found".into())
}

#[tauri::command]
async fn rewrite_selection(
    project_id: String,
    chapter_number: u32,
    selected_text: String,
    instruction: String,
    target_delta: u32,
    constraints: Option<Value>,
    api_format: String,
    api_key: String,
    model: String,
    base_url: String,
    proxy_url: Option<String>,
    user_agent: Option<String>,
) -> Result<Value, String> {
    // Input validation
    if chapter_number == 0 || chapter_number > 9999 {
        return Err("章节号必须在 1-9999 范围内".into());
    }
    if target_delta > 10000 {
        return Err("目标增量必须在 0-10000 范围内".into());
    }
    if selected_text.len() > 50000 {
        return Err("选中文本过长（最大 50000 字符）".into());
    }
    if instruction.len() > 5000 {
        return Err("指令过长（最大 5000 字符）".into());
    }

    let chapter_outline = find_chapter_outline(&project_id, chapter_number).unwrap_or_default();
    let world = storage::load_json(&project_id, "world.json")?.unwrap_or(json!({}));
    let chars = storage::load_json(&project_id, "characters.json")?.unwrap_or(json!({}));

    let context = {
        let raw = serde_json::to_string(&json!({
            "world": {"era": world["era"], "overview": truncate_chars(world["overview"].as_str().unwrap_or(""), 800)},
            "characters": chars["characters"].as_array().map(|a| a.iter().take(6).map(|c| json!({"name": c["name"], "role": c["role"], "personality": truncate_chars(c["personality"].as_str().unwrap_or(""), 200)})).collect::<Vec<_>>()).unwrap_or_default(),
        })).unwrap_or_default();
        truncate_chars(&raw, 3000).to_string()
    };
    let constraints_text = build_constraints_text(constraints.as_ref());
    let client = make_client(&api_format, &api_key, &model, &base_url, proxy_url, user_agent);
    // Truncate selected_text to prevent overflow
    let selected_truncated = truncate_chars(&selected_text, 3000);
    let system = "你是一位专业小说编辑。你的任务是只重写用户选中的局部片段，在保持上下文一致的前提下进行补写增强。只输出 JSON。";
    let user = format!(
        "{constraints}\n\n## 章节大纲\n{outline}\n\n## 全局上下文\n{ctx}\n\n## 用户选中的原文片段\n{sel}\n\n## 补写要求\n{instr}\n\n请只对上面的原文片段进行局部重写与扩写：\n1. 保持原意，不改变整体剧情方向\n2. 与章节上下文保持一致\n3. 在原片段基础上增加约 {delta} 字左右\n4. 只返回替换后的这一段，不要重写整章\n\n输出 JSON：{{\"text\":\"替换后的局部片段\"}}",
        constraints = truncate_chars(&constraints_text, 2000),
        outline = truncate_chars(&chapter_outline, 1000),
        ctx = context,
        sel = selected_truncated,
        instr = instruction,
        delta = target_delta,
    );
    client.generate_json(system, &user, 4096).await
}

// ===== RAG: Chapter Summary & Context =====

#[tauri::command]
async fn summarize_chapter(
    project_id: String,
    chapter_number: u32,
    api_format: String,
    api_key: String,
    model: String,
    base_url: String,
    proxy_url: Option<String>,
    user_agent: Option<String>,
) -> Result<Value, String> {
    let client = make_client(&api_format, &api_key, &model, &base_url, proxy_url, user_agent);
    auto_summarize_and_save(&client, &project_id, chapter_number).await
}

#[tauri::command]
async fn get_chapter_summaries(project_id: String) -> Result<Value, String> {
    Ok(storage::load_json(&project_id, "chapter_summaries.json")?.unwrap_or(json!({})))
}

/// Build rich context for chapter generation by retrieving relevant previous chapter summaries
#[tauri::command]
async fn build_chapter_context(
    project_id: String,
    chapter_number: u32,
) -> Result<Value, String> {
    let world = storage::load_json(&project_id, "world.json")?.unwrap_or(json!({}));
    let chars = storage::load_json(&project_id, "characters.json")?.unwrap_or(json!({}));
    let summaries = storage::load_json(&project_id, "chapter_summaries.json")?.unwrap_or(json!({}));

    // Collect summaries for previous chapters
    let mut prev_summaries: Vec<String> = Vec::new();
    let mut character_states: Vec<Value> = Vec::new();
    let mut active_foreshadowing: Vec<String> = Vec::new();
    let mut resolved_foreshadowing: Vec<String> = Vec::new();

    for ch_num in 1..chapter_number {
        let key = ch_num.to_string();
        if let Some(s) = summaries.get(&key) {
            // Add compact summary
            let summary_text = s["summary"].as_str().unwrap_or("");
            if !summary_text.is_empty() {
                prev_summaries.push(format!("第{}章：{}", ch_num, summary_text));
            }
            // Collect character changes
            if let Some(changes) = s["character_changes"].as_array() {
                for c in changes {
                    character_states.push(json!({
                        "chapter": ch_num,
                        "name": c["name"],
                        "change": c["change"]
                    }));
                }
            }
            // Collect foreshadowing
            if let Some(planted) = s["foreshadowing_planted"].as_array() {
                for f in planted {
                    if let Some(t) = f.as_str() {
                        active_foreshadowing.push(format!("第{}章埋设：{}", ch_num, t));
                    }
                }
            }
            if let Some(resolved) = s["foreshadowing_resolved"].as_array() {
                for f in resolved {
                    if let Some(t) = f.as_str() {
                        resolved_foreshadowing.push(t.to_string());
                    }
                }
            }
        }
    }

    // Remove resolved from active
    active_foreshadowing.retain(|f| {
        // Skip empty resolved entries: `contains("")` is always true and would
        // wipe every pending foreshadowing if the model emits an empty string.
        !resolved_foreshadowing.iter().any(|r| !r.is_empty() && f.contains(r))
    });

    // Get last chapter's end_state for continuity
    let last_end_state = if chapter_number > 1 {
        let prev_key = (chapter_number - 1).to_string();
        summaries.get(&prev_key)
            .and_then(|s| s["end_state"].as_str())
            .unwrap_or("")
            .to_string()
    } else {
        String::new()
    };

    // Build compact world + character context
    let world_brief = truncate_chars(world["overview"].as_str().unwrap_or(""), 500);
    let char_brief: Vec<Value> = chars["characters"].as_array()
        .map(|a| a.iter().take(6).map(|c| json!({
            "name": c["name"],
            "role": c["role"]
        })).collect())
        .unwrap_or_default();

    // Keep only recent summaries (last 10 chapters) + first 3 chapters
    let total = prev_summaries.len();
    let kept_summaries: Vec<String> = if total <= 13 {
        prev_summaries
    } else {
        let mut kept = prev_summaries[..3].to_vec();
        kept.push("...".to_string());
        kept.extend_from_slice(&prev_summaries[total - 10..]);
        kept
    };

    // Keep only recent character states (last 20)
    let states_len = character_states.len();
    let kept_states: Vec<Value> = if states_len <= 20 {
        character_states
    } else {
        character_states[states_len - 20..].to_vec()
    };

    Ok(json!({
        "world_brief": world_brief,
        "characters": char_brief,
        "previous_chapters": kept_summaries,
        "character_states": kept_states,
        "active_foreshadowing": active_foreshadowing,
        "last_chapter_end_state": last_end_state,
    }))
}

#[tauri::command]
async fn review_chapter(
    project_id: String,
    chapter_number: u32,
    chapter_text: String,
    platform: String,
    constraints: Option<Value>,
    api_format: String,
    api_key: String,
    model: String,
    base_url: String,
    proxy_url: Option<String>,
    user_agent: Option<String>,
) -> Result<String, String> {
    let meta = storage::load_json(&project_id, "meta.json")?.ok_or("Project not found")?;
    let genre = meta["genre"].as_str().unwrap_or("未知");
    let constraints_raw = build_constraints_text(constraints.as_ref());
    let constraints_text = truncate_chars(&constraints_raw, 2000);
    // Truncate chapter text to prevent token overflow (keep first 8000 chars for review)
    let chapter_truncated = truncate_chars(&chapter_text, 8000);

    let system = r#"你是一位拥有12年+网文行业从业经验的资深编辑，兼具小说创作指导与纵横、番茄、起点三大平台审核合规经验。你精通网文创作底层逻辑、读者情绪把控，精准掌握三大平台的内容审核红线、爆款偏好与排版规范。请对提供的章节进行全维度检查。"#;

    let user = format!(
        r#"{}

请对以下第{chapter_number}章内容进行专业审校检查。

小说类型：{genre}
目标平台：{platform}

## 章节内容
{chapter_truncated}

请按以下维度逐一检查并输出报告：

### 1. 逻辑检查
- 本章剧情推进是否合理，场景转换是否有支撑
- 人物行为、决策是否符合场景逻辑
- 时间线、空间设定是否有矛盾

### 2. 人设一致性
- 人物言行是否符合设定
- 人物互动是否自然

### 3. 剧情节奏
- 节奏是否张弛有度
- 爽点/情绪点是否到位
- 章节结尾钩子是否有效

### 4. 文字规范
- 语病、错别字、标点错误
- 叙事风格是否统一
- 段落划分是否合理

### 5. 平台合规性（{platform}）
- 是否有违规内容风险
- 敏感词筛查
- 字数是否适配平台要求

### 6. 优化建议
- 差异化亮点
- 具体可落地的修改方案

请对每个问题标注严重程度：🔴致命 🟡重要 🔵轻微
最后给出本章综合评分（满分100）和总结。"#,
        constraints_text
    );

    let client = make_client(&api_format, &api_key, &model, &base_url, proxy_url, user_agent);
    let msgs = vec![("user".to_string(), user)];
    client.chat(system, &msgs, 8192).await
}

// ===== Batch Chapter Generation =====

#[derive(Clone, Serialize)]
struct BatchProgress {
    current: u32,
    total: u32,
    chapter_number: u32,
    phase: String,
    word_count: u32,
    error: String,
}

#[derive(Clone, Serialize)]
struct BatchComplete {
    completed: u32,
    failed: u32,
    skipped: u32,
    total_words: u32,
    elapsed_seconds: u64,
    failed_chapters: Vec<u32>,
}

#[tauri::command]
async fn batch_generate_chapters(
    app: tauri::AppHandle,
    project_id: String,
    start_chapter: u32,
    end_chapter: u32,
    target_words: u32,
    skip_written: bool,
    constraints: Option<Value>,
    api_format: String,
    api_key: String,
    model: String,
    base_url: String,
    proxy_url: Option<String>,
    user_agent: Option<String>,
) -> Result<(), String> {
    // Input validation BEFORE acquiring lock
    if start_chapter == 0 || end_chapter == 0 || start_chapter > end_chapter {
        return Err("无效的章节范围".into());
    }
    if end_chapter - start_chapter > 100 {
        return Err("批量生成最多支持 100 章".into());
    }
    if target_words == 0 || target_words > 50000 {
        return Err("目标字数必须在 1-50000 范围内".into());
    }

    // Validate project exists before acquiring lock
    storage::load_json(&project_id, "meta.json")?
        .ok_or("Project not found")?;

    if BATCH_RUNNING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return Err("批量生成已在运行中".into());
    }

    // RAII guard resets BATCH_RUNNING exactly once when the worker task ends —
    // on normal completion OR panic (the guard is moved into the task below).
    struct ResetGuard;
    impl Drop for ResetGuard {
        fn drop(&mut self) {
            BATCH_RUNNING.store(false, Ordering::SeqCst);
        }
    }

    BATCH_CANCEL.store(false, Ordering::SeqCst);

    let constraints_clone = constraints.clone();
    let handle = tokio::spawn(async move {
        let _reset = ResetGuard;
        let start_time = std::time::Instant::now();
        let total = end_chapter - start_chapter + 1;
        let mut completed = 0u32;
        let mut failed = 0u32;
        let mut skipped = 0u32;
        let mut total_word_count = 0u32;
        let mut failed_chapters: Vec<u32> = Vec::new();

        let client = make_client(&api_format, &api_key, &model, &base_url, proxy_url, user_agent);

        // How a chapter's generation ended. Failed and Cancelled both stop the batch.
        enum ChapterOutcome { Done { word_count: u32 }, Failed(String), Cancelled }
        // A chapter must SUCCEED before the next one starts, so transient failures
        // are retried until they do. MAX_RETRIES is only a defensive ceiling so a
        // mis-classified permanent error can't loop forever — normal runs never hit it.
        const MAX_RETRIES: u32 = 50;

        for (idx, chapter_number) in (start_chapter..=end_chapter).enumerate() {
            // Check cancel
            if BATCH_CANCEL.load(Ordering::SeqCst) {
                let _ = app.emit("batch_progress", BatchProgress {
                    current: idx as u32 + 1, total, chapter_number,
                    phase: "cancelled".to_string(), word_count: 0, error: String::new(),
                });
                break;
            }

            // Skip written chapters
            if skip_written {
                let chapter_file = format!("chapter_{:03}.json", chapter_number);
                if let Ok(Some(existing)) = storage::load_json(&project_id, &chapter_file) {
                    let text = existing["text"].as_str().unwrap_or("");
                    if !text.is_empty() {
                        skipped += 1;
                        let _ = app.emit("batch_progress", BatchProgress {
                            current: idx as u32 + 1, total, chapter_number,
                            phase: "skipped".to_string(), word_count: text.chars().count() as u32, error: String::new(),
                        });
                        continue;
                    }
                }
            }

            // Phase: building context
            let _ = app.emit("batch_progress", BatchProgress {
                current: idx as u32 + 1, total, chapter_number,
                phase: "context".to_string(), word_count: 0, error: String::new(),
            });

            // Generate one chapter. A fatal failure (missing outline, generation
            // that fails even after retries, or a save error) STOPS the whole
            // batch — later chapters use this one's text + summary as RAG context,
            // so pressing on would only produce broken continuations.
            let chapter_file = format!("chapter_{:03}.json", chapter_number);
            let outcome: ChapterOutcome = 'chapter: {
                // Structural errors (missing context/outline) aren't retryable.
                let context = match build_rich_context_string(&project_id, chapter_number) {
                    Ok(c) => c,
                    Err(e) => break 'chapter ChapterOutcome::Failed(format!("构建上下文失败：{}", e)),
                };
                let chapter_outline = match find_chapter_outline(&project_id, chapter_number) {
                    Ok(o) => o,
                    Err(e) => break 'chapter ChapterOutcome::Failed(format!("获取大纲失败：{}", e)),
                };

                let constraints_text = build_constraints_text(constraints_clone.as_ref());
                let prompt = format!("{}\n\n{}", truncate_chars(&constraints_text, 2000), chapter_outline);

                // Generate, retrying transient failures with interruptible backoff.
                let chapter_data = {
                    let mut attempt = 0u32;
                    loop {
                        attempt += 1;
                        if BATCH_CANCEL.load(Ordering::SeqCst) {
                            break 'chapter ChapterOutcome::Cancelled;
                        }
                        let _ = app.emit("batch_progress", BatchProgress {
                            current: idx as u32 + 1, total, chapter_number,
                            phase: "generating".to_string(), word_count: 0, error: String::new(),
                        });
                        // Race generation against cancellation so a mid-chapter cancel
                        // takes effect at once instead of after the (minute-long) call.
                        let gen = tokio::select! {
                            biased;
                            _ = wait_cancel(&BATCH_CANCEL) => None,
                            r = engine::expand_chapter(&client, &prompt, "", target_words, &context) => Some(r),
                        };
                        match gen {
                            None => break 'chapter ChapterOutcome::Cancelled,
                            Some(Ok(data)) => break data,
                            Some(Err(e)) => {
                                // Keep retrying until this chapter succeeds — the next
                                // chapter can't start until it does. Only give up on
                                // errors retrying can't fix, or the defensive ceiling.
                                if batch_error_is_fatal(&e) {
                                    break 'chapter ChapterOutcome::Failed(format!("无法生成（重试也解决不了）：{}", e));
                                }
                                if attempt >= MAX_RETRIES {
                                    break 'chapter ChapterOutcome::Failed(format!("重试 {} 次仍失败，已停止：{}", MAX_RETRIES, e));
                                }
                                let _ = app.emit("batch_progress", BatchProgress {
                                    current: idx as u32 + 1, total, chapter_number,
                                    phase: "retrying".to_string(), word_count: 0,
                                    error: format!("[第 {} 次] {}", attempt, e),
                                });
                                // Backoff grows with attempts but is capped at 20s.
                                let backoff = (3 * attempt).min(20) as u64;
                                tokio::select! {
                                    biased;
                                    _ = wait_cancel(&BATCH_CANCEL) => break 'chapter ChapterOutcome::Cancelled,
                                    _ = tokio::time::sleep(Duration::from_secs(backoff)) => {}
                                }
                            }
                        }
                    }
                };

                // Snapshot + save under the project lock so a concurrent reorder or
                // autosave can't interleave; released before summarizing. Save error
                // is fatal (later chapters would build on a chapter that isn't there).
                let save_result = {
                    let lock = storage::project_lock(&project_id);
                    let _g = lock.lock().unwrap_or_else(|e| e.into_inner());
                    if let Ok(Some(existing)) = storage::load_json(&project_id, &chapter_file) {
                        if let Some(old) = existing["text"].as_str() {
                            if !old.is_empty() {
                                let _ = storage::save_snapshot(&project_id, chapter_number, old);
                            }
                        }
                    }
                    storage::save_json(&project_id, &chapter_file, &chapter_data)
                };
                if let Err(e) = save_result {
                    break 'chapter ChapterOutcome::Failed(format!("保存失败：{}", e));
                }

                let word_count = chapter_data["text"].as_str().map(|t| t.chars().count() as u32).unwrap_or(0);

                // Phase: summarizing — non-fatal (chapter text is already saved), but
                // still interruptible so cancel is responsive during summarization.
                let _ = app.emit("batch_progress", BatchProgress {
                    current: idx as u32 + 1, total, chapter_number,
                    phase: "summarizing".to_string(), word_count, error: String::new(),
                });
                let summ = tokio::select! {
                    biased;
                    _ = wait_cancel(&BATCH_CANCEL) => None,
                    r = auto_summarize_and_save(&client, &project_id, chapter_number) => Some(r),
                };
                match summ {
                    None => break 'chapter ChapterOutcome::Cancelled,
                    Some(Err(e)) => {
                        let _ = app.emit("batch_progress", BatchProgress {
                            current: idx as u32 + 1, total, chapter_number,
                            phase: "summarize_failed".to_string(), word_count, error: e,
                        });
                    }
                    Some(Ok(_)) => {}
                }

                ChapterOutcome::Done { word_count }
            };

            match outcome {
                ChapterOutcome::Done { word_count } => {
                    total_word_count += word_count;
                    completed += 1;
                    let _ = app.emit("batch_progress", BatchProgress {
                        current: idx as u32 + 1, total, chapter_number,
                        phase: "done".to_string(), word_count, error: String::new(),
                    });
                }
                ChapterOutcome::Cancelled => {
                    let _ = app.emit("batch_progress", BatchProgress {
                        current: idx as u32 + 1, total, chapter_number,
                        phase: "cancelled".to_string(), word_count: 0, error: String::new(),
                    });
                    break;
                }
                ChapterOutcome::Failed(error) => {
                    failed += 1;
                    failed_chapters.push(chapter_number);
                    let _ = app.emit("batch_progress", BatchProgress {
                        current: idx as u32 + 1, total, chapter_number,
                        phase: "failed".to_string(), word_count: 0, error,
                    });
                    // Stop the batch: don't build later chapters on a broken base.
                    break;
                }
            }
        }

        let elapsed = start_time.elapsed().as_secs();
        let _ = app.emit("batch_complete", BatchComplete {
            completed, failed, skipped,
            total_words: total_word_count,
            elapsed_seconds: elapsed,
            failed_chapters,
        });
    });

    // Watcher only logs a panic; the ResetGuard inside the task already resets
    // BATCH_RUNNING exactly once (on completion or panic), so resetting here too
    // could clear a *newer* batch's flag if one started in between.
    tokio::spawn(async move {
        if let Err(e) = handle.await {
            eprintln!("Batch generation task panicked: {:?}", e);
        }
    });

    Ok(())
}

#[tauri::command]
async fn cancel_batch_generation() -> Result<(), String> {
    BATCH_CANCEL.store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
async fn check_consistency(
    project_id: String,
    api_format: String,
    api_key: String,
    model: String,
    base_url: String,
    proxy_url: Option<String>,
    user_agent: Option<String>,
) -> Result<Value, String> {
    let summaries = storage::load_json(&project_id, "chapter_summaries.json")?.unwrap_or(json!({}));
    if summaries.as_object().map(|o| o.is_empty()).unwrap_or(true) {
        return Err("没有章节摘要数据，请先生成章节并运行摘要".into());
    }
    let world = storage::load_json(&project_id, "world.json")?.unwrap_or(json!({}));
    let chars = storage::load_json(&project_id, "characters.json")?.unwrap_or(json!({}));

    let summaries_str = serde_json::to_string_pretty(&summaries).unwrap_or_default();
    let world_str = format!("时代：{}\n概要：{}",
        world["era"].as_str().unwrap_or(""),
        truncate_chars(world["overview"].as_str().unwrap_or(""), 800),
    );
    let chars_str = chars["characters"].as_array()
        .map(|a| a.iter().take(8).map(|c| {
            format!("{}（{}）：{}", c["name"].as_str().unwrap_or(""), c["role"].as_str().unwrap_or(""), c["personality"].as_str().unwrap_or(""))
        }).collect::<Vec<_>>().join("\n"))
        .unwrap_or_default();

    let client = make_client(&api_format, &api_key, &model, &base_url, proxy_url, user_agent);
    engine::check_consistency(&client, &truncate_chars(&summaries_str, 6000), &world_str, &chars_str).await
}

// ===== Snapshot & Search =====

#[tauri::command]
async fn list_chapter_snapshots(project_id: String, chapter_number: u32) -> Result<Value, String> {
    let snapshots = storage::list_snapshots(&project_id, chapter_number)?;
    Ok(json!(snapshots))
}

#[tauri::command]
async fn restore_snapshot(project_id: String, chapter_number: u32, snapshot_file: String) -> Result<Value, String> {
    // Load snapshot
    let snap = storage::load_snapshot(&project_id, &snapshot_file)?;
    let snap_text = snap["text"].as_str().ok_or("快照内容为空")?;

    // Backup current before restoring
    let chapter_file = format!("chapter_{:03}.json", chapter_number);
    // Serialize the chapter file's read-modify-write (no .await while held).
    let lock = storage::project_lock(&project_id);
    let _g = lock.lock().unwrap_or_else(|e| e.into_inner());
    if let Ok(Some(existing)) = storage::load_json(&project_id, &chapter_file) {
        let old_text = existing["text"].as_str().unwrap_or("");
        if !old_text.is_empty() {
            let _ = storage::save_snapshot(&project_id, chapter_number, old_text);
        }
    }

    // Overwrite with snapshot
    let restored = json!({"text": snap_text});
    storage::save_json(&project_id, &chapter_file, &restored)?;
    Ok(restored)
}

#[tauri::command]
async fn search_chapters(project_id: String, query: String) -> Result<Value, String> {
    if query.is_empty() {
        return Ok(json!([]));
    }
    // Input validation
    if query.len() > 200 {
        return Err("搜索关键词过长（最大 200 字符）".into());
    }

    let query_lower = query.to_lowercase();
    let plot = storage::load_json(&project_id, "plot.json")?.unwrap_or(json!({}));

    let mut results: Vec<Value> = Vec::new();
    let mut total_matches = 0u32;

    // Timeout guard
    let search_start = std::time::Instant::now();
    let search_timeout = std::time::Duration::from_secs(10);

    // Scan all chapter files
    for num in 1..=9999u32 {
        if total_matches >= 100 { break; }
        if search_start.elapsed() > search_timeout {
            break; // Graceful timeout instead of error
        }
        let file = format!("chapter_{:03}.json", num);
        if let Ok(Some(ch)) = storage::load_json(&project_id, &file) {
            if let Some(text) = ch["text"].as_str() {
                let text_lower = text.to_lowercase();
                // Index entirely within `text_lower`: `to_lowercase()` can change
                // byte/char lengths (e.g. 'İ', 'K'→'k', 'ß'), so a byte offset from
                // text_lower must never index the original `text` (panic / mismatch).
                // The preview is lowercased as a result — acceptable for a snippet.
                let mut matches: Vec<Value> = Vec::new();
                let mut start = 0;
                while let Some(pos) = text_lower[start..].find(&query_lower) {
                    let abs_pos = start + pos;
                    let char_pos = text_lower[..abs_pos].chars().count();
                    let total_chars = text_lower.chars().count();
                    let ctx_char_start = char_pos.saturating_sub(30);
                    let query_char_len = query_lower.chars().count();
                    let ctx_char_end = (char_pos + query_char_len + 30).min(total_chars);
                    let cs = text_lower.char_indices().nth(ctx_char_start).map(|(i, _)| i).unwrap_or(0);
                    let ce = text_lower.char_indices().nth(ctx_char_end).map(|(i, _)| i).unwrap_or(text_lower.len());
                    matches.push(json!({
                        "offset": abs_pos,
                        "context": &text_lower[cs..ce],
                    }));
                    total_matches += 1;
                    if total_matches >= 100 { break; }
                    start = abs_pos + query_lower.len();
                }
                if !matches.is_empty() {
                    // Find chapter title from plot
                    let title = plot["acts"].as_array()
                        .and_then(|acts| acts.iter().find_map(|act| {
                            act["chapters"].as_array().and_then(|chs| {
                                chs.iter().find(|c| c["number"].as_u64() == Some(num as u64))
                                    .and_then(|c| c["title"].as_str())
                            })
                        }))
                        .unwrap_or("");
                    results.push(json!({
                        "chapter_number": num,
                        "title": title,
                        "matches": matches,
                    }));
                }
            }
        } else {
            continue; // Gap in chapter numbering, keep scanning
        }
    }
    Ok(json!(results))
}

// ===== Reader Simulation & Style Analysis =====

#[tauri::command]
async fn simulate_reader(
    project_id: String,
    chapter_number: u32,
    chapter_text: String,
    api_format: String,
    api_key: String,
    model: String,
    base_url: String,
    proxy_url: Option<String>,
    user_agent: Option<String>,
) -> Result<Value, String> {
    let chapter_outline = find_chapter_outline(&project_id, chapter_number).unwrap_or_default();
    let context = build_rich_context_string(&project_id, chapter_number)?;
    let client = make_client(&api_format, &api_key, &model, &base_url, proxy_url, user_agent);
    let truncated = truncate_chars(&chapter_text, 8000);
    engine::simulate_reader(&client, truncated, &chapter_outline, &truncate_chars(&context, 2000)).await
}

#[tauri::command]
async fn analyze_writing_style(
    project_id: String,
    api_format: String,
    api_key: String,
    model: String,
    base_url: String,
    proxy_url: Option<String>,
    user_agent: Option<String>,
) -> Result<Value, String> {
    // Collect up to 5 written chapters as samples
    let mut samples = Vec::new();
    for num in 1..=9999u32 {
        if samples.len() >= 5 { break; }
        let file = format!("chapter_{:03}.json", num);
        if let Ok(Some(ch)) = storage::load_json(&project_id, &file) {
            if let Some(text) = ch["text"].as_str() {
                if !text.is_empty() {
                    samples.push(format!("--- 第{}章样本 ---\n{}", num, truncate_chars(text, 2000)));
                }
            }
        }
    }
    if samples.len() < 3 {
        return Err("至少需要 3 章已写内容才能分析文风".into());
    }
    let combined = samples.join("\n\n");
    let client = make_client(&api_format, &api_key, &model, &base_url, proxy_url, user_agent);
    let result = engine::analyze_style(&client, &combined).await?;

    // Save style profile
    storage::save_json(&project_id, "style_profile.json", &result)?;
    Ok(result)
}

#[tauri::command]
async fn get_style_profile(project_id: String) -> Result<Value, String> {
    storage::load_json(&project_id, "style_profile.json")?
        .ok_or_else(|| "尚未分析文风".into())
}

#[tauri::command]
async fn sync_outline_from_chapter(
    project_id: String,
    chapter_number: u32,
    api_format: String,
    api_key: String,
    model: String,
    base_url: String,
    proxy_url: Option<String>,
    user_agent: Option<String>,
) -> Result<Value, String> {
    let chapter_file = format!("chapter_{:03}.json", chapter_number);
    let chapter = storage::load_json(&project_id, &chapter_file)?.ok_or("章节不存在")?;
    let chapter_text = chapter["text"].as_str().unwrap_or("");
    if chapter_text.is_empty() { return Err("章节内容为空".into()); }

    let client = make_client(&api_format, &api_key, &model, &base_url, proxy_url, user_agent);
    let result = engine::sync_outline(&client, chapter_text, chapter_number).await?;
    let new_summary = result["summary"].as_str().unwrap_or("");

    // Update plot.json
    if let Ok(Some(mut plot)) = storage::load_json(&project_id, "plot.json") {
        if let Some(acts) = plot["acts"].as_array_mut() {
            for act in acts.iter_mut() {
                if let Some(chapters) = act["chapters"].as_array_mut() {
                    for ch in chapters.iter_mut() {
                        if ch["number"].as_u64() == Some(chapter_number as u64) {
                            ch["summary"] = json!(new_summary);
                        }
                    }
                }
            }
        }
        storage::save_json(&project_id, "plot.json", &plot)?;
    }
    Ok(result)
}

#[tauri::command]
async fn generate_names(
    project_id: String,
    name_type: String,
    count: u32,
    api_format: String,
    api_key: String,
    model: String,
    base_url: String,
    proxy_url: Option<String>,
    user_agent: Option<String>,
) -> Result<Value, String> {
    // Input validation
    if count == 0 || count > 50 {
        return Err("生成数量必须在 1-50 范围内".into());
    }
    let allowed_types = ["character", "place", "skill", "item"];
    if !allowed_types.contains(&name_type.as_str()) {
        return Err("不支持的名字类型".into());
    }

    let world = storage::load_json(&project_id, "world.json")?.unwrap_or(json!({}));
    let world_summary = format!("时代：{}\n概要：{}",
        world["era"].as_str().unwrap_or(""),
        truncate_chars(world["overview"].as_str().unwrap_or(""), 800),
    );
    let client = make_client(&api_format, &api_key, &model, &base_url, proxy_url, user_agent);
    engine::generate_names(&client, &world_summary, &name_type, count).await
}

#[tauri::command]
async fn deep_sensitivity_check(
    chapter_text: String,
    api_format: String,
    api_key: String,
    model: String,
    base_url: String,
    proxy_url: Option<String>,
    user_agent: Option<String>,
) -> Result<Value, String> {
    let client = make_client(&api_format, &api_key, &model, &base_url, proxy_url, user_agent);
    engine::sensitivity_check(&client, &truncate_chars(&chapter_text, 8000)).await
}

fn sanitize_filename(s: &str, max_chars: usize) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' { c } else { '_' })
        .collect::<String>()
        .trim()
        .chars()
        .take(max_chars)
        .collect()
}

#[tauri::command]
async fn export_novel(project_id: String, format: String, mode: Option<String>) -> Result<Value, String> {
    use std::io::Write as _;

    let per_chapter = mode.as_deref() == Some("chapters");
    log_export_project(&project_id, &format!("{}/{}", format, if per_chapter { "chapters" } else { "single" }));

    let meta = storage::load_json(&project_id, "meta.json")?.ok_or("Project not found")?;
    let title = meta["title"].as_str().unwrap_or("novel");

    // Validate format
    if format != "txt" && format != "md" && format != "html" {
        return Err("不支持的导出格式".into());
    }

    let plot = storage::load_json(&project_id, "plot.json")?;
    let mut chapter_nums: Vec<u32> = Vec::new();
    if let Some(plot) = &plot {
        if let Some(acts) = plot["acts"].as_array() {
            for act in acts {
                if let Some(chapters) = act["chapters"].as_array() {
                    for ch in chapters {
                        if let Some(num) = ch["number"].as_u64() {
                            chapter_nums.push(num as u32);
                        }
                    }
                }
            }
        }
    }
    if chapter_nums.is_empty() {
        // Scan for existing chapter files. Do NOT break on the first gap — a
        // missing chapter number in the middle must not truncate the export.
        for num in 1..=9999u32 {
            let file = format!("chapter_{:03}.json", num);
            if storage::load_json(&project_id, &file).ok().flatten().is_some() {
                chapter_nums.push(num);
            }
        }
    }

    let chapter_title_of = |num: u32| -> String {
        plot.as_ref()
            .and_then(|p| p["acts"].as_array())
            .and_then(|acts| {
                acts.iter().find_map(|act| {
                    act["chapters"].as_array().and_then(|chs| {
                        chs.iter().find_map(|c| {
                            if c["number"].as_u64() == Some(num as u64) {
                                c["title"].as_str().map(|s| s.to_string())
                            } else {
                                None
                            }
                        })
                    })
                })
            })
            .unwrap_or_default()
    };

    let export_dir = storage::project_dir(&project_id).join("exports");
    std::fs::create_dir_all(&export_dir).map_err(|e: std::io::Error| e.to_string())?;
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let safe_title = sanitize_filename(title, 100);
    let safe_title = if safe_title.is_empty() { "novel".to_string() } else { safe_title };

    let is_md = format == "md";
    let is_html = format == "html";

    // Per-chapter mode: one file per chapter inside a timestamped directory
    if per_chapter {
        let out_dir = export_dir.join(format!("{}_{}_分章", safe_title, timestamp));
        std::fs::create_dir_all(&out_dir).map_err(|e| format!("无法创建导出目录: {}", e))?;
        let mut count = 0usize;

        for num in &chapter_nums {
            let file = format!("chapter_{:03}.json", num);
            let Some(ch) = storage::load_json(&project_id, &file).ok().flatten() else { continue };
            let Some(text) = ch["text"].as_str() else { continue };
            if text.trim().is_empty() {
                continue;
            }
            let ch_title = chapter_title_of(*num);
            let safe_ch = sanitize_filename(&ch_title, 60);
            let filename = if safe_ch.is_empty() {
                format!("第{:03}章.{}", num, format)
            } else {
                format!("第{:03}章_{}.{}", num, safe_ch, format)
            };
            let heading = if ch_title.is_empty() {
                format!("第{}章", num)
            } else {
                format!("第{}章 {}", num, ch_title)
            };

            let f = std::fs::File::create(out_dir.join(&filename))
                .map_err(|e| format!("无法创建导出文件 {}: {}", filename, e))?;
            let mut w = std::io::BufWriter::new(f);
            if is_html {
                let head = format!(
                    "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>{t}</title><style>body{{max-width:800px;margin:40px auto;padding:0 20px;font-family:serif;line-height:1.8;color:#333}}h1{{text-align:center;margin-bottom:40px}}p{{text-indent:2em;margin:0.5em 0}}</style></head><body><h1>{t}</h1>\n",
                    t = html_escape(&heading)
                );
                w.write_all(head.as_bytes()).map_err(|e| e.to_string())?;
                for para in text.split('\n') {
                    let p = para.trim();
                    if !p.is_empty() {
                        let line = format!("<p>{}</p>\n", html_escape(p));
                        w.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
                    }
                }
                w.write_all(b"</body></html>").map_err(|e| e.to_string())?;
            } else if is_md {
                w.write_all(format!("# {}\n\n", heading).as_bytes()).map_err(|e| e.to_string())?;
                w.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
                w.write_all(b"\n").map_err(|e| e.to_string())?;
            } else {
                w.write_all(format!("{}\n\n", heading).as_bytes()).map_err(|e| e.to_string())?;
                w.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
                w.write_all(b"\n").map_err(|e| e.to_string())?;
            }
            w.flush().map_err(|e| format!("写入 {} 失败: {}", filename, e))?;
            count += 1;
        }

        if count == 0 {
            let _ = std::fs::remove_dir(&out_dir);
            return Err("没有可导出的章节内容".into());
        }
        return Ok(json!({ "path": out_dir.to_string_lossy(), "count": count }));
    }

    // Single-file mode: estimate total size to prevent OOM
    let mut estimated_size = 0usize;
    for num in &chapter_nums {
        let file = format!("chapter_{:03}.json", num);
        if let Ok(Some(chapter)) = storage::load_json(&project_id, &file) {
            if let Some(text) = chapter["text"].as_str() {
                estimated_size += text.len();
            }
        }
    }

    // Limit to 50MB
    if estimated_size > 50_000_000 {
        return Err("小说内容过大（超过 50MB），请使用按章导出".into());
    }

    let filename = format!("{}_{}.{}", safe_title, timestamp, format);
    let export_path = export_dir.join(&filename);

    let file = std::fs::File::create(&export_path)
        .map_err(|e| format!("无法创建导出文件: {}", e))?;
    let mut writer = std::io::BufWriter::new(file);

    // Write header
    if is_html {
        let header = format!(
            "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>{t}</title><style>body{{max-width:800px;margin:40px auto;padding:0 20px;font-family:serif;line-height:1.8;color:#333}}h1{{text-align:center;margin-bottom:40px}}h2{{margin-top:40px;border-bottom:1px solid #eee;padding-bottom:8px}}p{{text-indent:2em;margin:0.5em 0}}</style></head><body><h1>{t}</h1>\n",
            t = html_escape(title)
        );
        writer.write_all(header.as_bytes()).map_err(|e| e.to_string())?;
    } else if is_md {
        writer.write_all(format!("# {}\n\n", title).as_bytes()).map_err(|e| e.to_string())?;
    } else {
        writer.write_all(format!("\u{300A}{}\u{300B}\n\n", title).as_bytes()).map_err(|e| e.to_string())?;
    }

    // Write chapters incrementally
    let mut count = 0usize;
    for num in chapter_nums {
        let file = format!("chapter_{:03}.json", num);
        if let Ok(Some(ch)) = storage::load_json(&project_id, &file) {
            if let Some(text) = ch["text"].as_str() {
                let ch_title = chapter_title_of(num);

                if is_html {
                    let heading = if !ch_title.is_empty() {
                        format!("<h2>\u{7B2C}{}\u{7AE0} {}</h2>\n", num, html_escape(&ch_title))
                    } else {
                        format!("<h2>\u{7B2C}{}\u{7AE0}</h2>\n", num)
                    };
                    writer.write_all(heading.as_bytes()).map_err(|e| e.to_string())?;
                    for para in text.split('\n') {
                        let p = para.trim();
                        if !p.is_empty() {
                            let line = format!("<p>{}</p>\n", html_escape(p));
                            writer.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
                        }
                    }
                } else if is_md {
                    let heading = if !ch_title.is_empty() {
                        format!("## \u{7B2C}{}\u{7AE0} {}\n\n", num, ch_title)
                    } else {
                        format!("## \u{7B2C}{}\u{7AE0}\n\n", num)
                    };
                    writer.write_all(heading.as_bytes()).map_err(|e| e.to_string())?;
                    writer.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
                    writer.write_all(b"\n\n---\n\n").map_err(|e| e.to_string())?;
                } else {
                    let heading = if !ch_title.is_empty() {
                        format!("\u{7B2C}{}\u{7AE0} {}\n\n", num, ch_title)
                    } else {
                        format!("\u{7B2C}{}\u{7AE0}\n\n", num)
                    };
                    writer.write_all(heading.as_bytes()).map_err(|e| e.to_string())?;
                    writer.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
                    writer.write_all(b"\n\n").map_err(|e| e.to_string())?;
                }
                count += 1;
            }
        }
    }

    // Write footer
    if is_html {
        writer.write_all(b"</body></html>").map_err(|e| e.to_string())?;
    }

    writer.flush().map_err(|e| format!("刷新缓冲区失败: {}", e))?;
    if count == 0 {
        let _ = std::fs::remove_file(&export_path);
        return Err("没有可导出的章节内容".into());
    }
    Ok(json!({ "path": export_path.to_string_lossy(), "count": count }))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    storage::init_data_dir();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_data_dir,
            set_data_dir,
            test_llm,
            fetch_models,
            save_llm_config,
            get_llm_config,
            save_llm_profiles,
            get_llm_profiles,
            save_llm_providers,
            get_llm_providers,
            agent_chat,
            agent_chat_stream,
            cancel_agent_chat,
            chat_with_ai,
            extract_framework,
            create_project,
            list_projects,
            get_project,
            save_outline_source,
            get_outline_source,
            delete_project,
            generate_world,
            get_world,
            save_world_data,
            generate_characters,
            get_characters,
            save_characters_data,
            generate_plot,
            get_plot,
            save_plot_outline,
            generate_timeline,
            get_timeline,
            expand_chapter,
            continue_writing,
            save_chapter,
            swap_chapters,
            get_chapter,
            rewrite_selection,
            review_chapter,
            summarize_chapter,
            get_chapter_summaries,
            build_chapter_context,
            export_novel,
            batch_generate_chapters,
            cancel_batch_generation,
            check_consistency,
            list_chapter_snapshots,
            restore_snapshot,
            search_chapters,
            simulate_reader,
            analyze_writing_style,
            get_style_profile,
            sync_outline_from_chapter,
            generate_names,
            deep_sensitivity_check,
            list_skills,
            install_skill_repo,
            update_skill_repo,
            toggle_skill_repo,
            remove_skill_repo,
            get_skill_detail,
            read_skill_file,
            list_mcp_servers,
            install_mcp_repo,
            save_mcp_server,
            delete_mcp_server,
            test_mcp_server,
            start_mcp_server,
            stop_mcp_server,
            get_mcp_logs,
        ])
        .setup(|_app| {
            // Register cleanup handler for MCP servers on app exit
            Ok(())
        })
        .on_window_event(|_window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                // Cleanup all MCP servers when window closes
                plugins::mcp::cleanup_all_servers();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
