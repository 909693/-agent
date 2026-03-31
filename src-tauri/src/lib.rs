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
use tauri::Emitter;
use uuid::Uuid;

static BATCH_RUNNING: AtomicBool = AtomicBool::new(false);
static BATCH_CANCEL: AtomicBool = AtomicBool::new(false);

fn truncate_chars(s: &str, max: usize) -> &str {
    if s.len() <= max { return s; }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) { end -= 1; }
    &s[..end]
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
        if !prompts.is_empty() {
            let mut text = String::from("## 必须应用的提示词\n");
            for prompt in prompts.iter().take(3) {
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
fn find_chapter_outline(project_id: &str, chapter_number: u32) -> Result<String, String> {
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
fn build_rich_context_string(project_id: &str, chapter_number: u32) -> Result<String, String> {
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
        !resolved_foreshadowing.iter().any(|r| f.contains(r))
    });

    // Last chapter end_state
    let last_end_state = if chapter_number > 1 {
        summaries.get(&(chapter_number - 1).to_string())
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
    let mut summaries = storage::load_json(project_id, summaries_file)?.unwrap_or(json!({}));
    summaries[chapter_number.to_string()] = summary.clone();
    storage::save_json(project_id, summaries_file, &summaries)?;

    Ok(summary)
}

// --- Tauri Commands ---

#[tauri::command]
fn get_data_dir() -> String {
    storage::data_dir().to_string_lossy().to_string()
}

#[tauri::command]
async fn test_llm(api_format: String, api_key: String, model: String, base_url: String) -> Result<String, String> {
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
            let r1 = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .build().map_err(|e| format!("Client build: {:?}", e))?
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
            let r2 = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .build().map_err(|e| format!("Client build: {:?}", e))?
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
                url, body_str, status1, &text1[..text1.len().min(500)], status2, &text2[..text2.len().min(500)]))
        }
        _ => {
            let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
            let body = serde_json::json!({
                "model": model_name,
                "max_tokens": 50,
                "messages": [{"role": "user", "content": "Say hi"}]
            });
            let resp = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .build().map_err(|e| format!("Client build: {:?}", e))?
                .post(&url)
                .header("Authorization", format!("Bearer {}", api_key))
                .header("content-type", "application/json")
                .json(&body)
                .send().await.map_err(|e| format!("Request failed: {:?}", e))?;
            let status = resp.status().to_string();
            let text = resp.text().await.unwrap_or_default();
            Ok(format!("URL: {}\nStatus: {}\nResponse: {}", url, status, &text[..text.len().min(500)]))
        }
    }
}

#[tauri::command]
fn set_data_dir(new_dir: String, migrate: bool) -> Result<String, String> {
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
) -> Result<Value, String> {
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
你的回复必须是 JSON：
{{
  "reply": "给用户看的自然语言回复",
  "action": null 或 {{
    "type": "动作类型",
    "params": {{}}
  }}
}}

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
- 如果用户意图不明确，先追问再执行
- 生成类操作前要确认
- 回复要简洁友好
- reply 字段必须是中文"#
    );

    let client = make_client(&api_format, &api_key, &model, &base_url);
    let mut msgs = history.clone();
    msgs.push(("user".to_string(), message));

    let raw = client.chat(&system, &msgs, 4096).await?;

    // Try parse JSON, fallback to plain reply
    let trimmed = raw.trim();
    if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
        Ok(parsed)
    } else if trimmed.contains("```") {
        let start = trimmed.find("```").unwrap_or(0) + 3;
        let after = &trimmed[start..];
        let content_start = after.find('\n').unwrap_or(0) + 1;
        let end = after.rfind("```").unwrap_or(after.len());
        let json_str = &after[content_start..end];
        serde_json::from_str(json_str.trim()).unwrap_or_else(|_| json!({"reply": raw, "action": null}));
        Ok(serde_json::from_str(json_str.trim()).unwrap_or(json!({"reply": raw, "action": null})))
    } else if let Some(start) = trimmed.find('{') {
        let end = trimmed.rfind('}').unwrap_or(trimmed.len());
        Ok(serde_json::from_str(&trimmed[start..=end]).unwrap_or(json!({"reply": raw, "action": null})))
    } else {
        Ok(json!({"reply": raw, "action": null}))
    }
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
) -> Result<String, String> {
    let client = make_client(&api_format, &api_key, &model, &base_url);
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
) -> Result<Value, String> {
    let client = make_client(&api_format, &api_key, &model, &base_url);
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
        let start = trimmed.find("```").unwrap_or(0);
        let after_fence = &trimmed[start + 3..];
        let content_start = after_fence.find('\n').unwrap_or(0) + 1;
        let end = after_fence.rfind("```").unwrap_or(after_fence.len());
        &after_fence[content_start..end]
    } else if let Some(start) = trimmed.find('{') {
        let end = trimmed.rfind('}').unwrap_or(trimmed.len());
        &trimmed[start..=end]
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

fn make_client(api_format: &str, api_key: &str, model: &str, base_url: &str) -> LlmClient {
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
) -> Result<Value, String> {
    let meta = storage::load_json(&project_id, "meta.json")?.ok_or("Project not found")?;
    let outline_source = storage::load_json(&project_id, "outline_source.json")?.unwrap_or(json!({}));
    let client = make_client(&api_format, &api_key, &model, &base_url);
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

#[tauri::command]
async fn save_world_data(project_id: String, world: Value) -> Result<Value, String> {
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
) -> Result<Value, String> {
    let meta = storage::load_json(&project_id, "meta.json")?.ok_or("Project not found")?;
    let world = storage::load_json(&project_id, "world.json")?.ok_or("Generate world first")?;
    let outline_source = storage::load_json(&project_id, "outline_source.json")?.unwrap_or(json!({}));
    let client = make_client(&api_format, &api_key, &model, &base_url);
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
        &format!("{}\n\n{}", constraints_text, world_summary),
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
    storage::save_json(&project_id, "characters.json", &characters)?;
    Ok(characters)
}

#[tauri::command]
async fn generate_plot(
    project_id: String,
    constraints: Option<Value>,
    api_format: String,
    api_key: String,
    model: String,
    base_url: String,
) -> Result<Value, String> {
    let meta = storage::load_json(&project_id, "meta.json")?.ok_or("Project not found")?;
    let world = storage::load_json(&project_id, "world.json")?.ok_or("Generate world first")?;
    let chars =
        storage::load_json(&project_id, "characters.json")?.ok_or("Generate characters first")?;
    let outline_source = storage::load_json(&project_id, "outline_source.json")?.unwrap_or(json!({}));
    let client = make_client(&api_format, &api_key, &model, &base_url);
    let constraints_text = build_constraints_text(constraints.as_ref());
    let world_summary = serde_json::to_string(&json!({
        "era": world["era"], "overview": world["overview"]
    }))
    .unwrap_or_default();
    let chars_summary = serde_json::to_string(&chars["characters"]).unwrap_or_default();
    let plot = engine::generate_plot(
        &client,
        &format!("{}\n\n{}\n\n## 已导入大纲\n{}", constraints_text, meta["premise"].as_str().unwrap_or(""), truncate_chars(&serde_json::to_string_pretty(&outline_source).unwrap_or_default(), 3000)),
        meta["genre"].as_str().unwrap_or(""),
        meta["tone"].as_str().unwrap_or(""),
        &format!("{}\n\n{}", constraints_text, world_summary),
        &format!("{}\n\n{}", constraints_text, chars_summary),
    )
    .await?;
    storage::save_json(&project_id, "plot.json", &plot)?;
    Ok(plot)
}

#[tauri::command]
async fn get_plot(project_id: String) -> Result<Value, String> {
    storage::load_json(&project_id, "plot.json")?.ok_or_else(|| "Plot not generated yet".into())
}

#[tauri::command]
async fn save_plot_outline(project_id: String, plot: Value) -> Result<Value, String> {
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
) -> Result<Value, String> {
    let world = storage::load_json(&project_id, "world.json")?.ok_or("Generate world first")?;
    let plot = storage::load_json(&project_id, "plot.json")?.ok_or("Generate plot first")?;
    let client = make_client(&api_format, &api_key, &model, &base_url);
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
) -> Result<Value, String> {
    let chapter_outline = find_chapter_outline(&project_id, chapter_number)?;
    let mut context = build_rich_context_string(&project_id, chapter_number)?;
    // Inject style profile if available
    if let Ok(Some(style)) = storage::load_json(&project_id, "style_profile.json") {
        if let Some(summary) = style["summary"].as_str() {
            context.push_str(&format!("\n\n## 作者文风\n{}", summary));
        }
    }
    let constraints_text = build_constraints_text(constraints.as_ref());

    let client = make_client(&api_format, &api_key, &model, &base_url);
    let result = engine::expand_chapter(
        &client,
        &format!("{}\n\n{}", truncate_chars(&constraints_text, 2000), chapter_outline),
        &user_content,
        target_words,
        &context,
    )
    .await?;
    // Snapshot before overwriting
    let chapter_file = format!("chapter_{:03}.json", chapter_number);
    if let Ok(Some(existing)) = storage::load_json(&project_id, &chapter_file) {
        let old_text = existing["text"].as_str().unwrap_or("");
        if !old_text.is_empty() {
            let _ = storage::save_snapshot(&project_id, chapter_number, old_text);
        }
    }
    storage::save_json(&project_id, &chapter_file, &result)?;
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
) -> Result<Value, String> {
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
    let constraints_text = build_constraints_text(constraints.as_ref());    let client = make_client(&api_format, &api_key, &model, &base_url);
    // Truncate existing text to last 2000 chars to avoid token overflow
    let existing_tail = if existing_text.len() > 2000 { &existing_text[existing_text.len()-2000..] } else { existing_text };
    let result = engine::continue_writing(
        &client,
        existing_tail,
        &format!("{}\n\n{}", truncate_chars(&constraints_text, 2000), instruction),
        target_words,
        &context,
    )
    .await?;
    // Append to existing chapter
    let new_text = format!("{}{}", existing_text, result["text"].as_str().unwrap_or(""));
    let updated = json!({"text": new_text});
    storage::save_json(&project_id, &chapter_file, &updated)?;
    Ok(updated)
}

#[tauri::command]
async fn save_chapter(project_id: String, chapter_number: u32, text: String) -> Result<(), String> {
    // Auto-snapshot before overwriting
    let chapter_file = format!("chapter_{:03}.json", chapter_number);
    if let Ok(Some(existing)) = storage::load_json(&project_id, &chapter_file) {
        let old_text = existing["text"].as_str().unwrap_or("");
        if !old_text.is_empty() {
            let _ = storage::save_snapshot(&project_id, chapter_number, old_text);
        }
    }
    storage::save_json(&project_id, &chapter_file, &json!({"text": text}))
}

#[tauri::command]
async fn get_chapter(project_id: String, chapter_number: u32) -> Result<Value, String> {
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
) -> Result<Value, String> {
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
    let client = make_client(&api_format, &api_key, &model, &base_url);
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
) -> Result<Value, String> {
    let client = make_client(&api_format, &api_key, &model, &base_url);
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
        !resolved_foreshadowing.iter().any(|r| f.contains(r))
    });

    // Get last chapter's end_state for continuity
    let prev_key = (chapter_number - 1).to_string();
    let last_end_state = summaries.get(&prev_key)
        .and_then(|s| s["end_state"].as_str())
        .unwrap_or("")
        .to_string();

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

    let client = make_client(&api_format, &api_key, &model, &base_url);
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
) -> Result<(), String> {
    if BATCH_RUNNING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return Err("批量生成已在运行中".into());
    }
    BATCH_CANCEL.store(false, Ordering::SeqCst);

    let constraints_clone = constraints.clone();
    tokio::spawn(async move {
        let start_time = std::time::Instant::now();
        let total = end_chapter - start_chapter + 1;
        let mut completed = 0u32;
        let mut failed = 0u32;
        let mut skipped = 0u32;
        let mut total_word_count = 0u32;
        let mut failed_chapters: Vec<u32> = Vec::new();

        let client = make_client(&api_format, &api_key, &model, &base_url);

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
                            phase: "skipped".to_string(), word_count: text.len() as u32, error: String::new(),
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

            // Build context + outline
            let context = match build_rich_context_string(&project_id, chapter_number) {
                Ok(c) => c,
                Err(e) => {
                    failed += 1;
                    failed_chapters.push(chapter_number);
                    let _ = app.emit("batch_progress", BatchProgress {
                        current: idx as u32 + 1, total, chapter_number,
                        phase: "failed".to_string(), word_count: 0, error: e,
                    });
                    continue;
                }
            };

            let chapter_outline = match find_chapter_outline(&project_id, chapter_number) {
                Ok(o) => o,
                Err(e) => {
                    failed += 1;
                    failed_chapters.push(chapter_number);
                    let _ = app.emit("batch_progress", BatchProgress {
                        current: idx as u32 + 1, total, chapter_number,
                        phase: "failed".to_string(), word_count: 0, error: e,
                    });
                    continue;
                }
            };

            // Phase: generating
            let _ = app.emit("batch_progress", BatchProgress {
                current: idx as u32 + 1, total, chapter_number,
                phase: "generating".to_string(), word_count: 0, error: String::new(),
            });

            let constraints_text = build_constraints_text(constraints_clone.as_ref());
            let result = engine::expand_chapter(
                &client,
                &format!("{}\n\n{}", truncate_chars(&constraints_text, 2000), chapter_outline),
                "",
                target_words,
                &context,
            ).await;

            match result {
                Ok(chapter_data) => {
                    let chapter_file = format!("chapter_{:03}.json", chapter_number);
                    if let Err(e) = storage::save_json(&project_id, &chapter_file, &chapter_data) {
                        failed += 1;
                        failed_chapters.push(chapter_number);
                        let _ = app.emit("batch_progress", BatchProgress {
                            current: idx as u32 + 1, total, chapter_number,
                            phase: "failed".to_string(), word_count: 0, error: e,
                        });
                        continue;
                    }

                    let word_count = chapter_data["text"].as_str().map(|t| t.len() as u32).unwrap_or(0);
                    total_word_count += word_count;

                    // Phase: summarizing
                    let _ = app.emit("batch_progress", BatchProgress {
                        current: idx as u32 + 1, total, chapter_number,
                        phase: "summarizing".to_string(), word_count, error: String::new(),
                    });

                    // Auto-summarize for next chapter's RAG
                    if let Err(e) = auto_summarize_and_save(&client, &project_id, chapter_number).await {
                        // Summarization failure is non-fatal: chapter is saved, just log the error
                        let _ = app.emit("batch_progress", BatchProgress {
                            current: idx as u32 + 1, total, chapter_number,
                            phase: "summarize_failed".to_string(), word_count, error: e,
                        });
                    }

                    completed += 1;
                    let _ = app.emit("batch_progress", BatchProgress {
                        current: idx as u32 + 1, total, chapter_number,
                        phase: "done".to_string(), word_count, error: String::new(),
                    });
                }
                Err(e) => {
                    failed += 1;
                    failed_chapters.push(chapter_number);
                    let _ = app.emit("batch_progress", BatchProgress {
                        current: idx as u32 + 1, total, chapter_number,
                        phase: "failed".to_string(), word_count: 0, error: e,
                    });
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
        BATCH_RUNNING.store(false, Ordering::SeqCst);
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

    let client = make_client(&api_format, &api_key, &model, &base_url);
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
    let query_lower = query.to_lowercase();
    let plot = storage::load_json(&project_id, "plot.json")?.unwrap_or(json!({}));

    let mut results: Vec<Value> = Vec::new();
    let mut total_matches = 0u32;

    // Scan all chapter files
    for num in 1..=500u32 {
        if total_matches >= 100 { break; }
        let file = format!("chapter_{:03}.json", num);
        if let Ok(Some(ch)) = storage::load_json(&project_id, &file) {
            if let Some(text) = ch["text"].as_str() {
                let text_lower = text.to_lowercase();
                let mut matches: Vec<Value> = Vec::new();
                let mut start = 0;
                while let Some(pos) = text_lower[start..].find(&query_lower) {
                    let abs_pos = start + pos;
                    let ctx_start = abs_pos.saturating_sub(30);
                    let ctx_end = (abs_pos + query.len() + 30).min(text.len());
                    // Safe char boundaries
                    let mut cs = ctx_start;
                    while cs > 0 && !text.is_char_boundary(cs) { cs -= 1; }
                    let mut ce = ctx_end;
                    while ce < text.len() && !text.is_char_boundary(ce) { ce += 1; }
                    matches.push(json!({
                        "offset": abs_pos,
                        "context": &text[cs..ce],
                    }));
                    total_matches += 1;
                    if total_matches >= 100 { break; }
                    start = abs_pos + query.len();
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
            break; // No more chapters
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
) -> Result<Value, String> {
    let chapter_outline = find_chapter_outline(&project_id, chapter_number).unwrap_or_default();
    let context = build_rich_context_string(&project_id, chapter_number)?;
    let client = make_client(&api_format, &api_key, &model, &base_url);
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
) -> Result<Value, String> {
    // Collect up to 5 written chapters as samples
    let mut samples = Vec::new();
    for num in 1..=100u32 {
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
    let client = make_client(&api_format, &api_key, &model, &base_url);
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
async fn export_novel(project_id: String, format: String) -> Result<String, String> {
    let meta = storage::load_json(&project_id, "meta.json")?.ok_or("Project not found")?;
    let title = meta["title"].as_str().unwrap_or("novel");

    let mut full_text = String::new();
    full_text.push_str(&format!("\u{300A}{}\u{300B}\n\n", title));

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
        chapter_nums = (1..=100).collect();
    }
    for num in chapter_nums {
        let file = format!("chapter_{:03}.json", num);
        if let Ok(Some(ch)) = storage::load_json(&project_id, &file) {
            if let Some(text) = ch["text"].as_str() {
                let ch_title = plot
                    .as_ref()
                    .and_then(|p| {
                        p["acts"].as_array().and_then(|acts| {
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
                    })
                    .unwrap_or_default();

                if !ch_title.is_empty() {
                    full_text.push_str(&format!("\u{7B2C}{}\u{7AE0} {}\n\n", num, ch_title));
                } else {
                    full_text.push_str(&format!("\u{7B2C}{}\u{7AE0}\n\n", num));
                }
                full_text.push_str(text);
                full_text.push_str("\n\n");
            }
        }
    }
    if full_text.trim().lines().count() <= 1 {
        return Err(
            "\u{6CA1}\u{6709}\u{53EF}\u{5BFC}\u{51FA}\u{7684}\u{7AE0}\u{8282}\u{5185}\u{5BB9}"
                .into(),
        );
    }

    let desktop = dirs::desktop_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let filename = match format.as_str() {
        "txt" => format!("{}.txt", title),
        _ => format!("{}.txt", title),
    };
    let path = desktop.join(&filename);
    std::fs::write(&path, &full_text).map_err(|e| e.to_string())?;

    Ok(path.to_string_lossy().to_string())
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
            agent_chat,
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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
