use reqwest::{Client, Response};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tokio::time::sleep;

use crate::engine::tools::ToolDef;

// ===== Retry infrastructure =====

fn should_retry(status_code: u16) -> bool {
    matches!(status_code, 429 | 500 | 502 | 503 | 524 | 529)
}

fn exponential_backoff(attempt: u32) -> Duration {
    let base_ms = 1000u64;
    let max_ms = 30_000u64;
    let delay = (base_ms * 2u64.pow(attempt)).min(max_ms);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let jitter = (delay as f64 * 0.25 * ((nanos % 1000) as f64 / 500.0 - 1.0)) as i64;
    Duration::from_millis((delay as i64 + jitter).max(200) as u64)
}

fn retry_delay(status_code: u16, headers: &reqwest::header::HeaderMap, attempt: u32) -> Duration {
    if status_code == 429 {
        if let Some(ra) = headers.get("retry-after").and_then(|v| v.to_str().ok()) {
            if let Ok(secs) = ra.parse::<u64>() {
                return Duration::from_secs(secs.min(30));
            }
        }
    }
    exponential_backoff(attempt)
}

fn is_connection_error(e: &reqwest::Error) -> bool {
    e.is_connect() || e.is_timeout() || e.is_request()
}

// ===== Agent types =====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUseBlock {
    pub id: String,
    pub name: String,
    pub input: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    pub text: String,
    pub tool_uses: Vec<ToolUseBlock>,
    pub stop_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StreamEvent {
    #[serde(rename = "token")]
    Token { delta: String },
    #[serde(rename = "tool_call")]
    ToolCall { id: String, name: String, input: Value },
    #[serde(rename = "tool_result")]
    ToolResult { name: String, success: bool, result: String },
    #[serde(rename = "done")]
    Done { reply: String },
    #[serde(rename = "error")]
    Error { error: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role")]
pub enum AgentMsg {
    #[serde(rename = "user")]
    User { content: String },
    #[serde(rename = "assistant")]
    Assistant { text: String, tool_uses: Vec<ToolUseBlock> },
    #[serde(rename = "tool_result")]
    ToolResultMsg { tool_use_id: String, content: String },
}

/// Parse response robustly: bytes() first, then decode to text.
/// Handles SSE streaming responses (data: {...}\n) by reassembling chunks.
async fn parse_resp(resp: Response) -> Result<(bool, Value), String> {
    let status = resp.status();
    let ok = status.is_success();
    let url = resp.url().to_string();

    // Use streaming to read response body chunk by chunk
    // This keeps the connection alive and avoids intermediate proxy timeouts
    const MAX_RESP_BYTES: usize = 64 * 1024 * 1024; // 64MB hard cap to avoid OOM
    let mut body_bytes = Vec::new();
    let mut stream = resp.bytes_stream();
    let mut stream_broken = false;
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(bytes) => {
                body_bytes.extend_from_slice(&bytes);
                if body_bytes.len() > MAX_RESP_BYTES {
                    return Err(format!(
                        "响应体过大（超过 {}MB），已中止",
                        MAX_RESP_BYTES / 1024 / 1024
                    ));
                }
            }
            Err(e) => {
                // Genuine mid-body network failure. If nothing arrived yet, fail
                // outright. If partial data arrived, keep it only when the stream
                // already reached a completion marker (checked below) — otherwise
                // a truncated chapter would be persisted as if it were complete.
                if body_bytes.is_empty() {
                    return Err(format!(
                        "读取响应失败: {}\n请求URL: {}\nHTTP状态: {}",
                        e, redact_url_secrets(&url), status
                    ));
                }
                stream_broken = true;
                break;
            }
        }
    }

    // Try UTF-8 decode, fallback to lossy conversion
    let text = match String::from_utf8(body_bytes.clone()) {
        Ok(t) => t,
        Err(_) => String::from_utf8_lossy(&body_bytes).to_string(),
    };

    // Handle non-JSON error responses (e.g. "error code: 504")
    if !ok {
        eprintln!("[LlmClient] API error response (status {}): {}", status, text);
        if let Ok(data) = serde_json::from_str::<Value>(&text) {
            return Ok((false, data));
        }
        // Return raw text for debugging if not JSON (UTF-8 safe truncation)
        let preview = truncate_chars_for_preview(&text, 200);
        return Err(format!("API 错误 ({}): {}", status, preview));
    }

    if text.trim().is_empty() {
        return Err(format!("API 返回空响应 (URL: {})", redact_url_secrets(&url)));
    }

    // A broken stream is only acceptable if it already reached a completion
    // marker; otherwise reject rather than assemble a silently-truncated body.
    if stream_broken && !sse_stream_looks_complete(&text) {
        return Err(format!(
            "响应在传输中被截断（未收到结束标记），已丢弃不完整内容。URL: {}",
            redact_url_secrets(&url)
        ));
    }

    // Try direct JSON parse first
    if let Ok(data) = serde_json::from_str::<Value>(&text) {
        eprintln!("[LlmClient] Direct JSON parse success: {} bytes", text.len());
        return Ok((true, data));
    }

    // Detect SSE streaming response (lines starting with "data:" or "event:"
    let first_line = text.lines().next().unwrap_or("").trim();
    if first_line.starts_with("data:") || first_line.starts_with("event: ") {
        return parse_sse_to_openai_response(&text);
    }

    // Enhanced error message with response preview
    let preview = truncate_chars_for_preview(&text, 300);
    Err(format!(
        "API 响应格式无法解析\nURL: {}\n响应预览: {}",
        redact_url_secrets(&url), preview
    ))
}

/// Reassemble an SSE streaming response into a single Anthropic-compatible JSON object.
/// Handles: text_delta, input_json_delta (tool_use), thinking_delta, and OpenAI delta.content.
fn parse_sse_to_openai_response(sse_text: &str) -> Result<(bool, Value), String> {
    let mut full_content = String::new();
    let mut tool_json = String::new();
    let mut thinking_text = String::new();
    let mut model = String::new();
    let mut id = String::new();
    let mut stop_reason = String::new();
    let mut event_counts: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut delta_type_counts: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut last_error: Option<String> = None;

    for line in sse_text.lines() {
        let line = line.trim();
        if line.is_empty() || line == "data: [DONE]" || line.starts_with("event:") || line.starts_with(':') {
            continue;
        }
        let json_str = if let Some(pos) = line.find("data:") {
            let after_data = line[pos + 5..].trim();
            if after_data.starts_with('{') || after_data.starts_with('[') {
                after_data.to_string()
            } else {
                continue;
            }
        } else {
            continue;
        };
        let chunk: Value = match serde_json::from_str(&json_str) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let ev_type = chunk["type"].as_str().unwrap_or("unknown").to_string();
        *event_counts.entry(ev_type.clone()).or_insert(0) += 1;

        if model.is_empty() {
            if let Some(m) = chunk["message"]["model"].as_str().or_else(|| chunk["model"].as_str()) {
                model = m.to_string();
            }
            if let Some(i) = chunk["message"]["id"].as_str().or_else(|| chunk["id"].as_str()) {
                id = i.to_string();
            }
        }

        match ev_type.as_str() {
            "content_block_delta" => {
                let delta_type = chunk["delta"]["type"].as_str().unwrap_or("");
                *delta_type_counts.entry(delta_type.to_string()).or_insert(0) += 1;
                match delta_type {
                    "text_delta" | "text" | "" => {
                        if let Some(t) = chunk["delta"]["text"].as_str() {
                            full_content.push_str(t);
                        }
                    }
                    "input_json_delta" => {
                        if let Some(t) = chunk["delta"]["partial_json"].as_str() {
                            tool_json.push_str(t);
                        }
                    }
                    "thinking_delta" => {
                        if let Some(t) = chunk["delta"]["thinking"].as_str() {
                            thinking_text.push_str(t);
                        }
                    }
                    _ => {}
                }
            }
            "content_block_start" => {
                // tool_use blocks: capture pre-filled input if any
                if chunk["content_block"]["type"].as_str() == Some("tool_use") {
                    if let Some(input) = chunk["content_block"].get("input") {
                        if input.is_object() || input.is_array() {
                            let s = serde_json::to_string(input).unwrap_or_default();
                            if s != "{}" && s != "[]" {
                                tool_json.push_str(&s);
                            }
                        }
                    }
                }
                // text blocks: capture pre-filled text
                if chunk["content_block"]["type"].as_str() == Some("text") {
                    if let Some(t) = chunk["content_block"]["text"].as_str() {
                        full_content.push_str(t);
                    }
                }
            }
            "error" => {
                last_error = Some(chunk["error"].to_string());
            }
            "message_delta" => {
                if let Some(sr) = chunk["delta"]["stop_reason"].as_str() {
                    stop_reason = sr.to_string();
                }
            }
            _ => {}
        }

        // OpenAI-format chunk
        if let Some(delta_content) = chunk["choices"][0]["delta"]["content"].as_str() {
            full_content.push_str(delta_content);
        }
        // Some OpenAI-compat gateways put content in reasoning_content
        if let Some(reasoning) = chunk["choices"][0]["delta"]["reasoning_content"].as_str() {
            thinking_text.push_str(reasoning);
        }
        if stop_reason.is_empty() {
            if let Some(fr) = chunk["choices"][0]["finish_reason"].as_str() {
                stop_reason = if fr == "length" { "max_tokens".to_string() } else { fr.to_string() };
            }
        }
    }

    eprintln!(
        "[LlmClient] SSE parsed: {} raw bytes, events={:?}, deltas={:?}, text={}ch, tool_json={}ch, thinking={}ch",
        sse_text.len(), event_counts, delta_type_counts,
        full_content.chars().count(), tool_json.chars().count(), thinking_text.chars().count()
    );

    // Fallback order: text → tool_json → thinking
    let final_content = if !full_content.is_empty() {
        full_content
    } else if !tool_json.is_empty() {
        eprintln!("[LlmClient] SSE: no text_delta, using input_json_delta ({}ch)", tool_json.len());
        tool_json
    } else if !thinking_text.is_empty() {
        eprintln!("[LlmClient] SSE: only thinking_delta found, using as fallback ({}ch)", thinking_text.len());
        thinking_text
    } else {
        if let Some(err) = last_error {
            return Err(format!("SSE 流中收到错误事件: {}", err));
        }
        let preview = truncate_chars_for_preview(sse_text, 500);
        return Err(format!(
            "SSE 流响应中未找到有效内容 (events={:?}, deltas={:?}). SSE预览: {}",
            event_counts, delta_type_counts, preview
        ));
    };

    let mut assembled = serde_json::json!({
        "id": if id.is_empty() { "sse-assembled" } else { &id },
        "type": "message",
        "model": if model.is_empty() { "unknown" } else { &model },
        "content": [{
            "type": "text",
            "text": final_content
        }]
    });
    if !stop_reason.is_empty() {
        assembled["stop_reason"] = Value::String(stop_reason);
    }

    Ok((true, assembled))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub provider: String,
    pub api_key: String,
    pub model: String,
    pub base_url: String,
    #[serde(default)]
    pub accept_invalid_certs: bool,
    #[serde(default)]
    pub proxy_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAIRequest {
    model: String,
    max_tokens: u32,
    temperature: f32,
    messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<Value>,
    /// Explicitly disable streaming to avoid SSE chunked responses
    stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    system_instruction: Option<GeminiContent>,
    generation_config: Option<GeminiGenConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GeminiContent {
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<String>,
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GeminiPart {
    text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GeminiGenConfig {
    temperature: f32,
    max_output_tokens: u32,
    response_mime_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
}

pub struct LlmClient {
    http: Client,
    config: LlmConfig,
}

impl LlmClient {
    pub fn new(config: LlmConfig) -> Self {
        // Only accept invalid certs if explicitly enabled by user
        let accept_invalid = config.accept_invalid_certs;

        // Determine proxy first
        let proxy_source = if let Some(ref proxy_url) = config.proxy_url {
            let trimmed = proxy_url.trim();
            if trimmed.is_empty() {
                None
            } else {
                eprintln!("[LlmClient] Using proxy from config: {}", trimmed);
                Some(trimmed.to_string())
            }
        } else if let Ok(proxy_url) = std::env::var("HTTPS_PROXY")
            .or_else(|_| std::env::var("https_proxy"))
            .or_else(|_| std::env::var("HTTP_PROXY"))
            .or_else(|_| std::env::var("http_proxy"))
        {
            eprintln!("[LlmClient] Using proxy from environment: {}", proxy_url);
            Some(proxy_url)
        } else {
            eprintln!("[LlmClient] No proxy configured");
            None
        };

        // Build client: if explicit proxy is set, use it; otherwise use system defaults
        let mut builder = Client::builder()
            .danger_accept_invalid_certs(accept_invalid)
            .timeout(Duration::from_secs(600))
            .connect_timeout(Duration::from_secs(30))
            // Do not follow cross-host redirects: reqwest does not strip custom
            // auth headers (x-api-key / x-goog-api-key) on redirect, so following
            // one to another origin would leak the API key.
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() > 10 {
                    return attempt.error("too many redirects");
                }
                let same_host = attempt
                    .previous()
                    .last()
                    .and_then(|p| p.host_str())
                    .map(|prev| attempt.url().host_str() == Some(prev))
                    .unwrap_or(true);
                if same_host { attempt.follow() } else { attempt.stop() }
            }));

        if let Some(ref proxy_url) = proxy_source {
            if let Ok(proxy) = reqwest::Proxy::all(proxy_url) {
                eprintln!("[LlmClient] Proxy configured successfully");
                builder = builder.proxy(proxy);
            } else {
                eprintln!("[LlmClient] Failed to parse proxy URL: {}", proxy_url);
            }
        }

        Self {
            http: builder.build().unwrap_or_else(|e| {
                eprintln!("[LlmClient] Client build failed: {:?}, falling back to default", e);
                Client::new()
            }),
            config,
        }
    }

    fn claude_url(&self) -> String {
        let base = if self.config.base_url.is_empty() {
            "https://api.anthropic.com".to_string()
        } else {
            self.config.base_url.trim_end_matches('/').to_string()
        };
        format!("{base}/v1/messages")
    }

    fn openai_url(&self) -> String {
        let base = if self.config.base_url.is_empty() {
            "https://api.openai.com".to_string()
        } else {
            self.config.base_url.trim_end_matches('/').to_string()
        };
        format!("{base}/v1/chat/completions")
    }

    fn gemini_url(&self) -> String {
        let base = if self.config.base_url.is_empty() {
            "https://generativelanguage.googleapis.com".to_string()
        } else {
            self.config.base_url.trim_end_matches('/').to_string()
        };
        format!(
            "{base}/v1beta/models/{}:generateContent",
            self.config.model
        )
    }

    pub async fn generate_json(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        max_tokens: u32,
    ) -> Result<Value, String> {
        match self.config.provider.as_str() {
            "anthropic" => {
                self.call_claude(system_prompt, user_prompt, max_tokens)
                    .await
            }
            "openai" => {
                self.call_openai(system_prompt, user_prompt, max_tokens)
                    .await
            }
            "gemini" => {
                self.call_gemini(system_prompt, user_prompt, max_tokens)
                    .await
            }
            other => Err(format!("Unknown provider: {other}")),
        }
    }

    /// Multi-turn chat, returns plain text response
    pub async fn chat(
        &self,
        system_prompt: &str,
        messages: &[(String, String)], // Vec of (role, content)
        max_tokens: u32,
    ) -> Result<String, String> {
        match self.config.provider.as_str() {
            "anthropic" => self.chat_claude(system_prompt, messages, max_tokens).await,
            "openai" => self.chat_openai(system_prompt, messages, max_tokens).await,
            "gemini" => self.chat_gemini(system_prompt, messages, max_tokens).await,
            other => Err(format!("Unknown provider: {other}")),
        }
    }

    async fn chat_claude(
        &self,
        system_prompt: &str,
        messages: &[(String, String)],
        max_tokens: u32,
    ) -> Result<String, String> {
        // Anthropic requires at least one message
        let default_msgs;
        let effective_messages = if messages.is_empty() {
            default_msgs = vec![("user".to_string(), "请开始。".to_string())];
            &default_msgs[..]
        } else {
            messages
        };
        let msgs: Vec<Value> = effective_messages
            .iter()
            .map(|(role, content)| serde_json::json!({"role": role, "content": content}))
            .collect();
        let model_name = self.config.model
            .replace("-thinking", "")
            .replace("-cc", "");
        let body = serde_json::json!({
            "model": model_name,
            "max_tokens": max_tokens,
            "temperature": 0.7,
            "thinking": { "type": "disabled" },
            "stream": true,
            "system": system_prompt,
            "messages": msgs,
        });
        let url = self.claude_url();
        let resp = self
            .http
            .post(&url)
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Anthropic request failed ({}): {}", url, e))?;
        let (ok, data) = parse_resp(resp).await?;
        if !ok {
            return Err(format!("Anthropic API error: {}", data));
        }
        let text = extract_claude_text(&data)?;
        if data["stop_reason"].as_str() == Some("max_tokens") {
            eprintln!("[LlmClient] Warning: Claude response truncated (max_tokens), text len={}", text.len());
        }
        Ok(text)
    }

    async fn chat_openai(
        &self,
        system_prompt: &str,
        messages: &[(String, String)],
        max_tokens: u32,
    ) -> Result<String, String> {
        let mut msgs = vec![Message {
            role: "system".into(),
            content: system_prompt.to_string(),
        }];
        msgs.extend(messages.iter().map(|(role, content)| Message {
            role: role.clone(),
            content: content.clone(),
        }));
        let body = OpenAIRequest {
            model: self.config.model.clone(),
            max_tokens,
            temperature: 0.7,
            messages: msgs,
            response_format: None,
            stream: true,
        };
        let url = self.openai_url();
        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("OpenAI request failed ({}): {}", url, e))?;
        let (ok, data) = parse_resp(resp).await?;
        if !ok {
            return Err(format!("OpenAI API error: {}", data));
        }
        // SSE streaming response is reassembled by parse_resp → parse_sse_to_openai_response
        // Extract content from either assembled SSE or direct response
        if let Some(content) = data["content"][0]["text"].as_str() {
            Ok(content.to_string())
        } else if let Some(content) = data["choices"][0]["message"]["content"].as_str() {
            Ok(content.to_string())
        } else {
            Err(format!("No text in OpenAI response: {}", data))
        }
    }

    async fn chat_gemini(
        &self,
        system_prompt: &str,
        messages: &[(String, String)],
        max_tokens: u32,
    ) -> Result<String, String> {
        let contents: Vec<GeminiContent> = messages
            .iter()
            .map(|(role, content)| {
                let gemini_role = if role == "assistant" { "model" } else { "user" };
                GeminiContent {
                    role: Some(gemini_role.to_string()),
                    parts: vec![GeminiPart {
                        text: content.clone(),
                    }],
                }
            })
            .collect();
        let body = GeminiRequest {
            contents,
            system_instruction: Some(GeminiContent {
                role: None,
                parts: vec![GeminiPart {
                    text: system_prompt.to_string(),
                }],
            }),
            generation_config: Some(GeminiGenConfig {
                temperature: 0.7,
                max_output_tokens: max_tokens,
                response_mime_type: "text/plain".into(),
            }),
        };
        let url = self.gemini_url();
        let resp = self
            .http
            .post(&url)
            .header("x-goog-api-key", &self.config.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let (ok, data) = parse_resp(resp).await?;
        if !ok {
            return Err(format!("Gemini API error: {}", data));
        }
        data["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| format!("Gemini error: {}", data))
    }

    /// POST a JSON body with retry (connection errors + retryable statuses), then
    /// parse. Used by the non-streaming generation paths so a transient 429/5xx
    /// during batch generation retries instead of killing the chapter.
    async fn post_json_retry<B: Serialize>(
        &self,
        url: &str,
        headers: &[(&str, String)],
        body: &B,
    ) -> Result<(bool, Value), String> {
        let max_attempts = 4u32;
        let mut last_error = String::new();
        for attempt in 0..max_attempts {
            let mut req = self.http.post(url);
            for (k, v) in headers {
                req = req.header(*k, v.as_str());
            }
            match req.json(body).send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if !status.is_success() {
                        let code = status.as_u16();
                        if should_retry(code) && attempt + 1 < max_attempts {
                            let delay = retry_delay(code, resp.headers(), attempt);
                            last_error = format!("HTTP {}", code);
                            eprintln!("[LlmClient] HTTP {} (attempt {}), {}ms 后重试", code, attempt + 1, delay.as_millis());
                            sleep(delay).await;
                            continue;
                        }
                    }
                    return parse_resp(resp).await;
                }
                Err(e) => {
                    if is_connection_error(&e) && attempt + 1 < max_attempts {
                        let delay = exponential_backoff(attempt);
                        last_error = e.to_string();
                        eprintln!("[LlmClient] 连接错误 (attempt {}), {}ms 后重试: {}", attempt + 1, delay.as_millis(), e);
                        sleep(delay).await;
                        continue;
                    }
                    return Err(format!(
                        "请求失败 ({}): {:?}\n[DEBUG] proxy_url={:?}, base_url={}, model={}",
                        redact_url_secrets(url), e,
                        self.config.proxy_url.as_deref().map(redact_url_secrets),
                        self.config.base_url, self.config.model
                    ));
                }
            }
        }
        Err(format!("重试耗尽: {}", last_error))
    }

    async fn call_claude(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        max_tokens: u32,
    ) -> Result<Value, String> {
        // For JSON generation, strip "-thinking"/"-cc" to maximize output tokens
        let model_name = self.config.model
            .replace("-thinking", "")
            .replace("-cc", "");
        let body = serde_json::json!({
            "model": model_name,
            "max_tokens": max_tokens,
            "temperature": 0.3,
            "thinking": { "type": "disabled" },
            "stream": true,
            "system": system_prompt,
            "messages": [{"role": "user", "content": format!("{user_prompt}\n\nIMPORTANT: Return ONLY valid JSON, no internal thinking, no tool calls, no commentary. Direct JSON output only.")}],
        });
        let url = self.claude_url();

        let (ok, data) = self.post_json_retry(&url, &[
            ("x-api-key", self.config.api_key.clone()),
            ("anthropic-version", "2023-06-01".to_string()),
            ("content-type", "application/json".to_string()),
        ], &body).await?;
        if !ok {
            return Err(format!("Anthropic API error: {}", data));
        }
        if data["stop_reason"].as_str() == Some("max_tokens") {
            eprintln!("[LlmClient] Warning: JSON 响应因 max_tokens 截断，repair 可能丢数据");
        }
        let text = extract_claude_text(&data)?;
        parse_json_from_text(&text)
    }
    async fn call_openai(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        max_tokens: u32,
    ) -> Result<Value, String> {
        let body = OpenAIRequest {
            model: self.config.model.clone(),
            max_tokens,
            temperature: 0.3,
            messages: vec![
                Message {
                    role: "system".into(),
                    content: system_prompt.to_string(),
                },
                Message {
                    role: "user".into(),
                    content: format!("{user_prompt}\n\nReturn valid JSON."),
                },
            ],
            response_format: None,
            stream: true,
        };
        let url = self.openai_url();
        let prompt_len = system_prompt.len() + user_prompt.len();
        eprintln!("[LlmClient] Calling OpenAI API: {}", url);
        eprintln!("[LlmClient] Model: {}, max_tokens: {}, prompt_size: {} bytes",
                  self.config.model, max_tokens, prompt_len);

        let (ok, data) = self.post_json_retry(&url, &[
            ("Authorization", format!("Bearer {}", self.config.api_key)),
        ], &body).await?;
        if !ok {
            return Err(format!("OpenAI API error: {}", data));
        }
        // Handle both SSE-assembled and direct responses
        let text = if let Some(t) = data["content"][0]["text"].as_str() {
            t.to_string()
        } else if let Some(t) = data["choices"][0]["message"]["content"].as_str() {
            t.to_string()
        } else {
            return Err(format!("No text in OpenAI response: {}", data));
        };
        if data["stop_reason"].as_str() == Some("max_tokens") {
            eprintln!("[LlmClient] Warning: JSON 响应因 max_tokens 截断，repair 可能丢数据");
        }
        parse_json_from_text(&text)
    }

    async fn call_gemini(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        max_tokens: u32,
    ) -> Result<Value, String> {
        let body = GeminiRequest {
            contents: vec![GeminiContent {
                role: Some("user".into()),
                parts: vec![GeminiPart {
                    text: user_prompt.to_string(),
                }],
            }],
            system_instruction: Some(GeminiContent {
                role: None,
                parts: vec![GeminiPart {
                    text: system_prompt.to_string(),
                }],
            }),
            generation_config: Some(GeminiGenConfig {
                temperature: 0.3,
                max_output_tokens: max_tokens,
                response_mime_type: "application/json".into(),
            }),
        };
        let url = self.gemini_url();
        eprintln!("[LlmClient] Calling Gemini API: {}", url);
        eprintln!("[LlmClient] Model: {}, max_tokens: {}", self.config.model, max_tokens);

        let (ok, data) = self.post_json_retry(&url, &[
            ("x-goog-api-key", self.config.api_key.clone()),
        ], &body).await?;
        if !ok {
            return Err(format!("Gemini API error: {}", data));
        }
        let text = data["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .ok_or("No text in Gemini response")?;
        parse_json_from_text(text)
    }

    // ===== Agent streaming methods =====

    fn tools_to_anthropic(tools: &[ToolDef]) -> Vec<Value> {
        tools.iter().map(|t| serde_json::json!({
            "name": t.name,
            "description": t.description,
            "input_schema": t.parameters,
        })).collect()
    }

    fn tools_to_openai(tools: &[ToolDef]) -> Vec<Value> {
        tools.iter().map(|t| serde_json::json!({
            "type": "function",
            "function": {
                "name": t.name,
                "description": t.description,
                "parameters": t.parameters,
            }
        })).collect()
    }

    fn tools_to_gemini(tools: &[ToolDef]) -> Vec<Value> {
        tools.iter().map(|t| serde_json::json!({
            "name": t.name,
            "description": t.description,
            "parameters": t.parameters,
        })).collect()
    }

    fn build_anthropic_messages(msgs: &[AgentMsg]) -> Vec<Value> {
        let mut result = Vec::new();
        for msg in msgs {
            match msg {
                AgentMsg::User { content } => {
                    result.push(serde_json::json!({"role": "user", "content": content}));
                }
                AgentMsg::Assistant { text, tool_uses } => {
                    let mut content: Vec<Value> = Vec::new();
                    if !text.is_empty() {
                        content.push(serde_json::json!({"type": "text", "text": text}));
                    }
                    for tu in tool_uses {
                        content.push(serde_json::json!({
                            "type": "tool_use",
                            "id": tu.id,
                            "name": tu.name,
                            "input": tu.input,
                        }));
                    }
                    if content.is_empty() {
                        content.push(serde_json::json!({"type": "text", "text": " "}));
                    }
                    result.push(serde_json::json!({"role": "assistant", "content": content}));
                }
                AgentMsg::ToolResultMsg { tool_use_id, content } => {
                    result.push(serde_json::json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": tool_use_id,
                            "content": content,
                        }]
                    }));
                }
            }
        }
        result
    }

    fn build_openai_messages(system: &str, msgs: &[AgentMsg]) -> Vec<Value> {
        let mut result = vec![serde_json::json!({"role": "system", "content": system})];
        for msg in msgs {
            match msg {
                AgentMsg::User { content } => {
                    result.push(serde_json::json!({"role": "user", "content": content}));
                }
                AgentMsg::Assistant { text, tool_uses } => {
                    if tool_uses.is_empty() {
                        result.push(serde_json::json!({"role": "assistant", "content": text}));
                    } else {
                        let tool_calls: Vec<Value> = tool_uses.iter().map(|tu| serde_json::json!({
                            "id": tu.id,
                            "type": "function",
                            "function": {
                                "name": tu.name,
                                "arguments": serde_json::to_string(&tu.input).unwrap_or_default(),
                            }
                        })).collect();
                        let mut m = serde_json::json!({"role": "assistant"});
                        if !text.is_empty() {
                            m["content"] = Value::String(text.clone());
                        }
                        m["tool_calls"] = Value::Array(tool_calls);
                        result.push(m);
                    }
                }
                AgentMsg::ToolResultMsg { tool_use_id, content } => {
                    result.push(serde_json::json!({
                        "role": "tool",
                        "tool_call_id": tool_use_id,
                        "content": content,
                    }));
                }
            }
        }
        result
    }

    fn build_gemini_contents(msgs: &[AgentMsg]) -> Vec<Value> {
        // Map tool_use_id -> function name so each tool result carries the correct
        // functionResponse.name (Gemini matches results to calls by name, not id).
        let mut id_to_name: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for msg in msgs {
            if let AgentMsg::Assistant { tool_uses, .. } = msg {
                for tu in tool_uses {
                    id_to_name.insert(tu.id.clone(), tu.name.clone());
                }
            }
        }

        // Flush accumulated tool results as a single user turn (Gemini expects all
        // functionResponse parts for one model turn grouped into one content).
        fn flush(result: &mut Vec<Value>, pending: &mut Vec<Value>) {
            if !pending.is_empty() {
                result.push(serde_json::json!({"role": "user", "parts": pending.clone()}));
                pending.clear();
            }
        }

        let mut result: Vec<Value> = Vec::new();
        let mut pending_responses: Vec<Value> = Vec::new();
        for msg in msgs {
            match msg {
                AgentMsg::User { content } => {
                    flush(&mut result, &mut pending_responses);
                    result.push(serde_json::json!({
                        "role": "user",
                        "parts": [{"text": content}]
                    }));
                }
                AgentMsg::Assistant { text, tool_uses } => {
                    flush(&mut result, &mut pending_responses);
                    let mut parts: Vec<Value> = Vec::new();
                    if !text.is_empty() {
                        parts.push(serde_json::json!({"text": text}));
                    }
                    for tu in tool_uses {
                        parts.push(serde_json::json!({
                            "functionCall": {"name": tu.name, "args": tu.input}
                        }));
                    }
                    if parts.is_empty() {
                        parts.push(serde_json::json!({"text": " "}));
                    }
                    result.push(serde_json::json!({"role": "model", "parts": parts}));
                }
                AgentMsg::ToolResultMsg { tool_use_id, content } => {
                    let parsed: Value = serde_json::from_str(content).unwrap_or(serde_json::json!({"result": content}));
                    let name = id_to_name.get(tool_use_id).cloned().unwrap_or_else(|| "tool".to_string());
                    pending_responses.push(serde_json::json!({
                        "functionResponse": {"name": name, "response": parsed}
                    }));
                }
            }
        }
        flush(&mut result, &mut pending_responses);
        result
    }

    pub async fn chat_with_tools_stream(
        &self,
        system: &str,
        messages: &[AgentMsg],
        tools: &[ToolDef],
        max_tokens: u32,
        on_token: impl Fn(&str) + Send + Sync,
    ) -> Result<AgentResponse, String> {
        match self.config.provider.as_str() {
            "anthropic" => self.stream_anthropic(system, messages, tools, max_tokens, on_token).await,
            "openai" => self.stream_openai(system, messages, tools, max_tokens, on_token).await,
            "gemini" => self.stream_gemini(system, messages, tools, max_tokens, on_token).await,
            other => Err(format!("Unknown provider: {}", other)),
        }
    }

    async fn stream_anthropic(
        &self,
        system: &str,
        messages: &[AgentMsg],
        tools: &[ToolDef],
        max_tokens: u32,
        on_token: impl Fn(&str) + Send + Sync,
    ) -> Result<AgentResponse, String> {
        let model_name = self.config.model.replace("-thinking", "").replace("-cc", "");
        let msgs = Self::build_anthropic_messages(messages);
        let tool_defs = Self::tools_to_anthropic(tools);
        let url = self.claude_url();
        let mut last_error = String::new();
        let mut drop_tools = false;
        let max_attempts = 4u32;

        for attempt in 0..max_attempts {
            let use_tools = !drop_tools;
            let mut body = serde_json::json!({
                "model": model_name,
                "max_tokens": max_tokens,
                "temperature": 0.7,
                "thinking": {"type": "disabled"},
                "stream": true,
                "system": system,
                "messages": msgs,
            });
            if use_tools && !tool_defs.is_empty() {
                body["tools"] = Value::Array(tool_defs.clone());
            }

            let resp = match self.http.post(&url)
                .header("x-api-key", &self.config.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&body)
                .send().await
            {
                Ok(r) => r,
                Err(e) => {
                    if is_connection_error(&e) && attempt + 1 < max_attempts {
                        let delay = exponential_backoff(attempt);
                        eprintln!("[Agent/Anthropic] 连接错误 (attempt {}), {}ms 后重试: {}", attempt + 1, delay.as_millis(), e);
                        sleep(delay).await;
                        continue;
                    }
                    return Err(format!("Anthropic request failed: {}", e));
                }
            };

            if !resp.status().is_success() {
                let status_code = resp.status().as_u16();
                let headers = resp.headers().clone();
                let body_text = resp.text().await.unwrap_or_default();

                if status_code == 524 && !drop_tools && !tool_defs.is_empty() {
                    eprintln!("[Agent/Anthropic] 524 timeout with tools, retrying without tools...");
                    drop_tools = true;
                    continue;
                }

                if should_retry(status_code) && attempt + 1 < max_attempts {
                    let delay = retry_delay(status_code, &headers, attempt);
                    eprintln!("[Agent/Anthropic] HTTP {} (attempt {}), {}ms 后重试", status_code, attempt + 1, delay.as_millis());
                    last_error = format!("HTTP {}: {}", status_code, truncate_chars_for_preview(&body_text, 200));
                    sleep(delay).await;
                    continue;
                }

                return Err(format!("Anthropic API error ({}): {}", status_code, truncate_chars_for_preview(&body_text, 300)));
            }

            return self.process_anthropic_sse(resp, &on_token).await;
        }
        Err(format!("Anthropic API: 重试耗尽 - {}", last_error))
    }

    async fn process_anthropic_sse(
        &self,
        resp: Response,
        on_token: &(dyn Fn(&str) + Send + Sync),
    ) -> Result<AgentResponse, String> {
        let mut full_text = String::new();
        let mut tool_uses: Vec<ToolUseBlock> = Vec::new();
        let mut current_tool_id = String::new();
        let mut current_tool_name = String::new();
        let mut current_tool_json = String::new();
        let mut byte_buf: Vec<u8> = Vec::new();
        let mut stop_reason: Option<String> = None;

        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let bytes = chunk.map_err(|e| format!("Stream error: {}", e))?;
            byte_buf.extend_from_slice(&bytes);
            if byte_buf.len() > 64 * 1024 * 1024 {
                return Err("流响应过大（超过 64MB），已中止".to_string());
            }

            // Split on '\n' at the byte level (a newline byte never occurs inside
            // a UTF-8 multi-byte sequence), so multi-byte chars are never cut
            // across chunks and each complete line decodes losslessly.
            while let Some(newline_pos) = byte_buf.iter().position(|&b| b == b'\n') {
                let line_bytes: Vec<u8> = byte_buf.drain(..=newline_pos).collect();
                let line = String::from_utf8_lossy(&line_bytes);
                let line = line.trim();

                if line.is_empty() || line == "data: [DONE]" || line.starts_with("event:") || line.starts_with(':') {
                    continue;
                }
                let json_str = if let Some(pos) = line.find("data:") {
                    let after = line[pos + 5..].trim();
                    if after.starts_with('{') { after.to_string() } else { continue; }
                } else { continue; };

                let chunk: Value = match serde_json::from_str(&json_str) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                let ev_type = chunk["type"].as_str().unwrap_or("");
                match ev_type {
                    "content_block_start" => {
                        if chunk["content_block"]["type"].as_str() == Some("tool_use") {
                            current_tool_id = chunk["content_block"]["id"].as_str().unwrap_or("").to_string();
                            current_tool_name = chunk["content_block"]["name"].as_str().unwrap_or("").to_string();
                            current_tool_json.clear();
                        }
                    }
                    "content_block_delta" => {
                        let delta_type = chunk["delta"]["type"].as_str().unwrap_or("");
                        match delta_type {
                            "text_delta" => {
                                if let Some(t) = chunk["delta"]["text"].as_str() {
                                    full_text.push_str(t);
                                    on_token(t);
                                }
                            }
                            "input_json_delta" => {
                                if let Some(t) = chunk["delta"]["partial_json"].as_str() {
                                    current_tool_json.push_str(t);
                                }
                            }
                            _ => {}
                        }
                    }
                    "content_block_stop" => {
                        if !current_tool_name.is_empty() {
                            let input: Value = serde_json::from_str(&current_tool_json).unwrap_or(serde_json::json!({}));
                            tool_uses.push(ToolUseBlock {
                                id: current_tool_id.clone(),
                                name: current_tool_name.clone(),
                                input,
                            });
                            current_tool_name.clear();
                            current_tool_json.clear();
                        }
                    }
                    "error" => {
                        return Err(format!("Anthropic stream error: {}", chunk["error"]));
                    }
                    "message_delta" => {
                        if let Some(sr) = chunk["delta"]["stop_reason"].as_str() {
                            stop_reason = Some(sr.to_string());
                        }
                    }
                    _ => {}
                }
            }
        }

        Ok(AgentResponse { text: full_text, tool_uses, stop_reason })
    }

    async fn stream_openai(
        &self,
        system: &str,
        messages: &[AgentMsg],
        tools: &[ToolDef],
        max_tokens: u32,
        on_token: impl Fn(&str) + Send + Sync,
    ) -> Result<AgentResponse, String> {
        let msgs = Self::build_openai_messages(system, messages);
        let tool_defs = Self::tools_to_openai(tools);
        let url = self.openai_url();
        let mut last_error = String::new();
        let mut drop_tools = false;
        let max_attempts = 4u32;

        for attempt in 0..max_attempts {
            let use_tools = !drop_tools;
            let mut body = serde_json::json!({
                "model": self.config.model,
                "max_tokens": max_tokens,
                "temperature": 0.7,
                "stream": true,
                "messages": msgs,
            });
            if use_tools && !tool_defs.is_empty() {
                body["tools"] = Value::Array(tool_defs.clone());
            }

            let resp = match self.http.post(&url)
                .header("Authorization", format!("Bearer {}", self.config.api_key))
                .header("content-type", "application/json")
                .json(&body)
                .send().await
            {
                Ok(r) => r,
                Err(e) => {
                    if is_connection_error(&e) && attempt + 1 < max_attempts {
                        let delay = exponential_backoff(attempt);
                        eprintln!("[Agent/OpenAI] 连接错误 (attempt {}), {}ms 后重试: {}", attempt + 1, delay.as_millis(), e);
                        sleep(delay).await;
                        continue;
                    }
                    return Err(format!("OpenAI request failed: {}", e));
                }
            };

            if !resp.status().is_success() {
                let status_code = resp.status().as_u16();
                let headers = resp.headers().clone();
                let body_text = resp.text().await.unwrap_or_default();

                if status_code == 524 && !drop_tools && !tool_defs.is_empty() {
                    eprintln!("[Agent/OpenAI] 524 timeout with tools, retrying without tools...");
                    drop_tools = true;
                    continue;
                }

                if should_retry(status_code) && attempt + 1 < max_attempts {
                    let delay = retry_delay(status_code, &headers, attempt);
                    eprintln!("[Agent/OpenAI] HTTP {} (attempt {}), {}ms 后重试", status_code, attempt + 1, delay.as_millis());
                    last_error = format!("HTTP {}: {}", status_code, truncate_chars_for_preview(&body_text, 200));
                    sleep(delay).await;
                    continue;
                }

                return Err(format!("OpenAI API error ({}): {}", status_code, truncate_chars_for_preview(&body_text, 300)));
            }

            return self.process_openai_sse(resp, &on_token).await;
        }
        Err(format!("OpenAI API: 重试耗尽 - {}", last_error))
    }

    async fn process_openai_sse(
        &self,
        resp: Response,
        on_token: &(dyn Fn(&str) + Send + Sync),
    ) -> Result<AgentResponse, String> {
        let mut full_text = String::new();
        let mut tool_calls: std::collections::HashMap<u32, (String, String, String)> = std::collections::HashMap::new();
        let mut byte_buf: Vec<u8> = Vec::new();
        let mut stop_reason: Option<String> = None;

        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let bytes = chunk.map_err(|e| format!("Stream error: {}", e))?;
            byte_buf.extend_from_slice(&bytes);
            if byte_buf.len() > 64 * 1024 * 1024 {
                return Err("流响应过大（超过 64MB），已中止".to_string());
            }

            // Split on '\n' at the byte level so multi-byte chars are never cut
            // across chunks; decode each complete line losslessly.
            while let Some(newline_pos) = byte_buf.iter().position(|&b| b == b'\n') {
                let line_bytes: Vec<u8> = byte_buf.drain(..=newline_pos).collect();
                let line = String::from_utf8_lossy(&line_bytes);
                let line = line.trim();

                if line.is_empty() || line == "data: [DONE]" { continue; }
                let json_str = if let Some(pos) = line.find("data:") {
                    let after = line[pos + 5..].trim();
                    if after.starts_with('{') { after.to_string() } else { continue; }
                } else { continue; };

                let chunk: Value = match serde_json::from_str(&json_str) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                // Text delta
                if let Some(content) = chunk["choices"][0]["delta"]["content"].as_str() {
                    full_text.push_str(content);
                    on_token(content);
                }

                // Finish reason
                if let Some(fr) = chunk["choices"][0]["finish_reason"].as_str() {
                    stop_reason = Some(match fr {
                        "length" => "max_tokens".to_string(),
                        other => other.to_string(),
                    });
                }

                // Tool calls
                if let Some(tcs) = chunk["choices"][0]["delta"]["tool_calls"].as_array() {
                    for tc in tcs {
                        let idx = tc["index"].as_u64().unwrap_or(0) as u32;
                        let entry = tool_calls.entry(idx).or_insert_with(|| (String::new(), String::new(), String::new()));
                        if let Some(id) = tc["id"].as_str() {
                            entry.0 = id.to_string();
                        }
                        if let Some(name) = tc["function"]["name"].as_str() {
                            entry.1 = name.to_string();
                        }
                        if let Some(args) = tc["function"]["arguments"].as_str() {
                            entry.2.push_str(args);
                        }
                    }
                }
            }
        }

        // Emit tool calls in the provider's index order — HashMap iteration is
        // nondeterministic, which would randomize write-tool execution order.
        let mut ordered: Vec<(u32, (String, String, String))> = tool_calls.into_iter().collect();
        ordered.sort_by_key(|(idx, _)| *idx);
        let tool_uses: Vec<ToolUseBlock> = ordered.into_iter().map(|(_, (id, name, args))| {
            let input: Value = serde_json::from_str(&args).unwrap_or(serde_json::json!({}));
            ToolUseBlock { id, name, input }
        }).collect();

        Ok(AgentResponse { text: full_text, tool_uses, stop_reason })
    }

    async fn stream_gemini(
        &self,
        system: &str,
        messages: &[AgentMsg],
        tools: &[ToolDef],
        max_tokens: u32,
        on_token: impl Fn(&str) + Send + Sync,
    ) -> Result<AgentResponse, String> {
        let contents = Self::build_gemini_contents(messages);
        let tool_defs = Self::tools_to_gemini(tools);
        let base = if self.config.base_url.is_empty() {
            "https://generativelanguage.googleapis.com".to_string()
        } else {
            self.config.base_url.trim_end_matches('/').to_string()
        };
        // API key goes in the x-goog-api-key header, never in the URL (URLs leak
        // into error messages and stderr logs).
        let url = format!("{}/v1beta/models/{}:streamGenerateContent?alt=sse", base, self.config.model);
        let mut last_error = String::new();
        let mut drop_tools = false;
        let max_attempts = 4u32;

        for attempt in 0..max_attempts {
            let use_tools = !drop_tools;
            let mut body = serde_json::json!({
                "contents": contents,
                "system_instruction": {"parts": [{"text": system}]},
                "generation_config": {
                    "temperature": 0.7,
                    "max_output_tokens": max_tokens,
                },
            });
            if use_tools && !tool_defs.is_empty() {
                body["tools"] = serde_json::json!([{"functionDeclarations": tool_defs}]);
            }

            let resp = match self.http.post(&url)
                .header("content-type", "application/json")
                .header("x-goog-api-key", &self.config.api_key)
                .json(&body)
                .send().await
            {
                Ok(r) => r,
                Err(e) => {
                    if is_connection_error(&e) && attempt + 1 < max_attempts {
                        let delay = exponential_backoff(attempt);
                        eprintln!("[Agent/Gemini] 连接错误 (attempt {}), {}ms 后重试: {}", attempt + 1, delay.as_millis(), e);
                        sleep(delay).await;
                        continue;
                    }
                    return Err(format!("Gemini request failed: {}", e));
                }
            };

            if !resp.status().is_success() {
                let status_code = resp.status().as_u16();
                let headers = resp.headers().clone();
                let body_text = resp.text().await.unwrap_or_default();

                if status_code == 524 && !drop_tools && !tool_defs.is_empty() {
                    eprintln!("[Agent/Gemini] 524 timeout with tools, retrying without tools...");
                    drop_tools = true;
                    continue;
                }

                if should_retry(status_code) && attempt + 1 < max_attempts {
                    let delay = retry_delay(status_code, &headers, attempt);
                    eprintln!("[Agent/Gemini] HTTP {} (attempt {}), {}ms 后重试", status_code, attempt + 1, delay.as_millis());
                    last_error = format!("HTTP {}: {}", status_code, truncate_chars_for_preview(&body_text, 200));
                    sleep(delay).await;
                    continue;
                }

                return Err(format!("Gemini API error ({}): {}", status_code, truncate_chars_for_preview(&body_text, 300)));
            }

            return self.process_gemini_sse(resp, &on_token).await;
        }
        Err(format!("Gemini API: 重试耗尽 - {}", last_error))
    }

    async fn process_gemini_sse(
        &self,
        resp: Response,
        on_token: &(dyn Fn(&str) + Send + Sync),
    ) -> Result<AgentResponse, String> {
        let mut full_text = String::new();
        let mut tool_uses: Vec<ToolUseBlock> = Vec::new();
        let mut byte_buf: Vec<u8> = Vec::new();
        let mut stop_reason: Option<String> = None;

        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let bytes = chunk.map_err(|e| format!("Stream error: {}", e))?;
            byte_buf.extend_from_slice(&bytes);
            if byte_buf.len() > 64 * 1024 * 1024 {
                return Err("流响应过大（超过 64MB），已中止".to_string());
            }

            // Split on '\n' at the byte level so multi-byte chars are never cut
            // across chunks; decode each complete line losslessly.
            while let Some(newline_pos) = byte_buf.iter().position(|&b| b == b'\n') {
                let line_bytes: Vec<u8> = byte_buf.drain(..=newline_pos).collect();
                let line = String::from_utf8_lossy(&line_bytes);
                let line = line.trim();

                if line.is_empty() || line.starts_with("event:") { continue; }
                let json_str = if let Some(pos) = line.find("data:") {
                    let after = line[pos + 5..].trim();
                    if after.starts_with('{') { after.to_string() } else { continue; }
                } else { continue; };

                let chunk: Value = match serde_json::from_str(&json_str) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                if let Some(parts) = chunk["candidates"][0]["content"]["parts"].as_array() {
                    for part in parts {
                        if let Some(t) = part["text"].as_str() {
                            full_text.push_str(t);
                            on_token(t);
                        }
                        if let Some(fc) = part.get("functionCall") {
                            let name = fc["name"].as_str().unwrap_or("").to_string();
                            let args = fc["args"].clone();
                            tool_uses.push(ToolUseBlock {
                                id: format!("gemini_{}", tool_uses.len()),
                                name,
                                input: args,
                            });
                        }
                    }
                }
                if let Some(fr) = chunk["candidates"][0]["finishReason"].as_str() {
                    stop_reason = Some(match fr {
                        "MAX_TOKENS" => "max_tokens".to_string(),
                        other => other.to_lowercase(),
                    });
                }
            }
        }

        Ok(AgentResponse { text: full_text, tool_uses, stop_reason })
    }
}

/// Extract text from Claude response content array.
/// Handles both text blocks and tool_use blocks (extracts tool input as JSON text).
fn extract_claude_text(data: &Value) -> Result<String, String> {
    let content = data["content"].as_array()
        .ok_or_else(|| format!("No content array in Claude response: {}", data))?;

    // First, try to find text blocks
    let mut texts: Vec<String> = Vec::new();
    for block in content {
        if block["type"].as_str() == Some("text") {
            if let Some(t) = block["text"].as_str() {
                texts.push(t.to_string());
            }
        }
    }
    if !texts.is_empty() {
        return Ok(texts.join("\n"));
    }

    // If no text blocks, check for tool_use blocks and extract input as JSON
    for block in content {
        if block["type"].as_str() == Some("tool_use") {
            if let Some(input) = block.get("input") {
                return Ok(serde_json::to_string(input).unwrap_or_default());
            }
        }
    }

    // Fallback: stringify entire content
    Err(format!("No text in Claude response: {}", data))
}

fn parse_json_from_text(text: &str) -> Result<Value, String> {
    let trimmed = text.trim();
    // Strip markdown code fences if present
    let json_str = if trimmed.starts_with("```") {
        let len = trimmed.len();
        // Content starts after the first newline (skip the ```lang marker line);
        // clamp to len so a fence with no newline (e.g. just "```") can't overflow.
        let start = trimmed.find('\n').map(|i| i + 1).unwrap_or(len).min(len);
        // Closing fence, searched only within the content after the opening fence.
        let end = trimmed[start..].rfind("```").map(|i| start + i).unwrap_or(len);
        if end > start {
            &trimmed[start..end]
        } else {
            &trimmed[start..]
        }
    } else {
        trimmed
    };
    // Also try to find JSON object/array within the text
    let json_str = json_str.trim();
    if let Ok(val) = serde_json::from_str::<Value>(&json_str) {
        return Ok(val);
    }
    // Fallback: find first { or [ and last } or ]
    if let Some(obj_start) = json_str.find('{') {
        if let Some(obj_end) = json_str.rfind('}') {
            if obj_end > obj_start {
                if let Ok(val) = serde_json::from_str(&json_str[obj_start..=obj_end]) {
                    return Ok(val);
                }
            }
        }
        // Try to repair truncated JSON by closing open brackets
        let fragment = &json_str[obj_start..];
        if let Ok(val) = repair_truncated_json(fragment) {
            return Ok(val);
        }
    }
    let preview = truncate_chars_for_preview(text, 500);
    Err(format!("JSON parse error (len={}): {}", text.len(), preview))
}

/// Attempt to repair truncated JSON by walking byte-by-byte with a proper
/// state machine, then truncating to the last safe position and closing
/// open containers. Handles escapes inside strings correctly.
fn repair_truncated_json(text: &str) -> Result<Value, String> {
    let bytes = text.as_bytes();
    let mut stack: Vec<u8> = Vec::new(); // open containers: b'{' or b'['
    let mut in_string = false;
    let mut escape = false;
    // Last byte offset at which the JSON was in a structurally safe state
    // (not inside a string, not inside a number/literal being read, and with
    // a non-empty stack so we still have something to close). We resume here
    // on truncation. A "safe" boundary is right after `,` `[` `{` `:` or
    // right after a complete value (closing `}` `]` `"`, digit end).
    let mut safe_end: usize = 0;
    let mut in_number = false;
    let mut after_value = false; // just closed a string/array/object/number

    for (i, &c) in bytes.iter().enumerate() {
        if in_string {
            if escape {
                escape = false;
            } else if c == b'\\' {
                escape = true;
            } else if c == b'"' {
                in_string = false;
                after_value = true;
                safe_end = i + 1;
            }
            continue;
        }

        // Not in string
        if in_number {
            let is_num_char = c.is_ascii_digit()
                || c == b'.' || c == b'-' || c == b'+'
                || c == b'e' || c == b'E';
            if is_num_char {
                continue;
            } else {
                in_number = false;
                after_value = true;
                safe_end = i; // number ends just before this byte
                // fall through to handle current byte
            }
        }

        match c {
            b'"' => {
                in_string = true;
                escape = false;
                after_value = false;
            }
            b'{' | b'[' => {
                stack.push(c);
                after_value = false;
                if !stack.is_empty() {
                    safe_end = i + 1;
                }
            }
            b'}' => {
                if stack.last() == Some(&b'{') {
                    stack.pop();
                    after_value = true;
                    if !stack.is_empty() {
                        safe_end = i + 1;
                    }
                }
            }
            b']' => {
                if stack.last() == Some(&b'[') {
                    stack.pop();
                    after_value = true;
                    if !stack.is_empty() {
                        safe_end = i + 1;
                    }
                }
            }
            b',' => {
                after_value = false;
                if !stack.is_empty() {
                    safe_end = i + 1;
                }
            }
            b':' => {
                after_value = false;
                if !stack.is_empty() {
                    safe_end = i + 1;
                }
            }
            b' ' | b'\t' | b'\n' | b'\r' => {
                if after_value && !stack.is_empty() {
                    safe_end = i + 1;
                }
            }
            b'0'..=b'9' | b'-' => {
                in_number = true;
                after_value = false;
            }
            b't' | b'f' | b'n' => {
                // Literal true/false/null — find its end
                let end = match c {
                    b't' | b'f' if bytes.get(i..i + 4) == Some(b"true") => i + 4,
                    b'f' if bytes.get(i..i + 5) == Some(b"false") => i + 5,
                    b'n' if bytes.get(i..i + 4) == Some(b"null") => i + 4,
                    _ => i,
                };
                if end > i {
                    after_value = true;
                    safe_end = end;
                }
            }
            _ => {}
        }
    }

    // Truncate to last known-safe position, or handle in-string truncation
    let mut repaired = if safe_end > 0 && safe_end <= text.len() {
        text[..safe_end].to_string()
    } else if in_string && !stack.is_empty() {
        // Truncated inside a string value — close the string and containers
        let mut s = text.to_string();
        // Remove trailing incomplete escape sequence
        if s.ends_with('\\') { s.pop(); }
        s.push('"');
        s
    } else {
        text.to_string()
    };

    // Strip trailing commas/colons/whitespace that would break parsing
    let trim_trailing = |s: &mut String| {
        while let Some(c) = s.chars().last() {
            if c == ',' || c == ':' || c.is_whitespace() {
                s.pop();
            } else {
                break;
            }
        }
    };
    trim_trailing(&mut repaired);

    // Recompute stack depth on the truncated string (it may differ from
    // the full-text walk because we cut off some trailing opens).
    let mut final_stack: Vec<u8> = Vec::new();
    let mut s_in_str = false;
    let mut s_esc = false;
    for &c in repaired.as_bytes() {
        if s_in_str {
            if s_esc { s_esc = false; }
            else if c == b'\\' { s_esc = true; }
            else if c == b'"' { s_in_str = false; }
            continue;
        }
        match c {
            b'"' => s_in_str = true,
            b'{' | b'[' => final_stack.push(c),
            b'}' if final_stack.last() == Some(&b'{') => { final_stack.pop(); }
            b']' if final_stack.last() == Some(&b'[') => { final_stack.pop(); }
            _ => {}
        }
    }

    // Close open containers from innermost outward
    while let Some(opener) = final_stack.pop() {
        repaired.push(if opener == b'{' { '}' } else { ']' });
    }

    serde_json::from_str(&repaired).map_err(|e| {
        format!(
            "JSON repair failed after truncating to {} bytes: {}",
            repaired.len(), e
        )
    })
}

/// Redact secrets that may appear in a URL before logging or surfacing to the UI:
/// the `key=` query parameter value (Gemini) and any `user:pass@` userinfo.
fn redact_url_secrets(url: &str) -> String {
    let mut out = url.to_string();
    // Redact key=... query value
    if let Some(pos) = out.find("key=") {
        let start = pos + 4;
        let end = out[start..].find('&').map(|i| start + i).unwrap_or(out.len());
        out.replace_range(start..end, "***");
    }
    // Redact userinfo credentials in scheme://user:pass@host
    if let Some(scheme_pos) = out.find("://") {
        let after = scheme_pos + 3;
        let path_at = out[after..].find('/').map(|i| after + i).unwrap_or(out.len());
        if let Some(at_rel) = out[after..path_at].find('@') {
            let at = after + at_rel;
            out.replace_range(after..at, "***");
        }
    }
    out
}

/// Heuristic: does an SSE/text body already contain a stream-completion marker?
/// Used to decide whether a mid-stream network error truncated the response.
fn sse_stream_looks_complete(text: &str) -> bool {
    if !text.contains("data:") {
        // Not an SSE stream — downstream JSON parsing validates completeness.
        return true;
    }
    text.contains("message_stop")
        || text.contains("[DONE]")
        || text.contains("\"stop_reason\"")
        || text.contains("\"finish_reason\"")
        || text.contains("\"finishReason\"")
}

/// Truncate string to at most `max_chars` characters, respecting UTF-8 boundaries.
fn truncate_chars_for_preview(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut chars = s.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    format!("{}...[{}+{} chars]", truncated, truncated.chars().count(), s.chars().count() - max_chars)
}
