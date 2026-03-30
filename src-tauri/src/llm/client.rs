use reqwest::{Client, Response};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

/// Parse response robustly: text() first, then JSON parse
async fn parse_resp(resp: Response) -> Result<(bool, Value), String> {
    let status = resp.status();
    let ok = status.is_success();
    let text = resp.text().await.map_err(|e| format!("读取响应失败: {}", e))?;

    // Handle non-JSON error responses (e.g. "error code: 504")
    if !ok {
        if let Ok(data) = serde_json::from_str::<Value>(&text) {
            return Ok((false, data));
        }
        return Err(format!("API 错误 ({}): {}", status, text.chars().take(300).collect::<String>()));
    }

    if text.trim().is_empty() {
        return Err("API 返回空响应".to_string());
    }

    let data: Value = serde_json::from_str(&text)
        .map_err(|e| format!("JSON 解析失败: {} (响应前200字: {})", e, &text[..text.len().min(200)]))?;
    Ok((true, data))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub provider: String,
    pub api_key: String,
    pub model: String,
    pub base_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClaudeRequest {
    model: String,
    max_tokens: u32,
    temperature: f32,
    system: String,
    messages: Vec<Message>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAIRequest {
    model: String,
    max_tokens: u32,
    temperature: f32,
    messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<Value>,
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
        Self {
            http: Client::builder()
                .danger_accept_invalid_certs(true)
                .timeout(Duration::from_secs(300))
                .connect_timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| Client::new()),
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
            "{base}/v1beta/models/{}:generateContent?key={}",
            self.config.model, self.config.api_key
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
        let body = serde_json::json!({
            "model": self.config.model,
            "max_tokens": max_tokens,
            "temperature": 0.7,
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
        extract_claude_text(&data)
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
        data["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| format!("No text in OpenAI response: {}", data))
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

    async fn call_claude(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        max_tokens: u32,
    ) -> Result<Value, String> {
        let body = serde_json::json!({
            "model": self.config.model,
            "max_tokens": max_tokens,
            "temperature": 0.3,
            "system": system_prompt,
            "messages": [{"role": "user", "content": format!("{user_prompt}\n\nIMPORTANT: Return ONLY valid JSON, no tool calls.")}],
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
            response_format: Some(serde_json::json!({"type": "json_object"})),
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
        let text = data["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| format!("No text in OpenAI response: {}", data))?;
        parse_json_from_text(text)
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
        let url = format!(
            "{}/v1beta/models/{}:generateContent?key={}",
            if self.config.base_url.is_empty() {
                "https://generativelanguage.googleapis.com"
            } else {
                self.config.base_url.trim_end_matches('/')
            },
            self.config.model,
            self.config.api_key
        );
        let resp = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let (ok, data) = parse_resp(resp).await?;
        if !ok {
            return Err(format!("Gemini API error: {}", data));
        }
        let text = data["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .ok_or("No text in Gemini response")?;
        parse_json_from_text(text)
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
        let start = trimmed.find('\n').unwrap_or(3) + 1;
        let end = trimmed.rfind("```").unwrap_or(trimmed.len());
        &trimmed[start..end]
    } else {
        trimmed
    };
    serde_json::from_str(json_str.trim()).map_err(|e| format!("JSON parse error: {e}"))
}
