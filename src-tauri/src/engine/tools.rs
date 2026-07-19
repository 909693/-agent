use serde_json::{json, Value};
use crate::storage;
use crate::engine;
use crate::llm::client::LlmClient;

pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Value,
    #[allow(dead_code)]
    pub is_read_only: bool,
}

pub fn is_tool_read_only(name: &str) -> bool {
    matches!(name,
        "get_project_info" | "get_world" | "get_characters"
        | "get_chapter_outline" | "get_plot_outline" | "get_chapter"
        | "get_chapter_summaries" | "search_chapters"
    )
}

pub fn get_tool_definitions() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "get_project_info",
            description: "获取当前项目的基本信息（标题、类型、基调、前提、主题等）",
            parameters: json!({"type": "object", "properties": {}, "required": []}),
            is_read_only: true,
        },
        ToolDef {
            name: "get_world",
            description: "获取当前项目的世界观设定",
            parameters: json!({"type": "object", "properties": {}, "required": []}),
            is_read_only: true,
        },
        ToolDef {
            name: "get_characters",
            description: "获取当前项目的角色列表及详情",
            parameters: json!({"type": "object", "properties": {}, "required": []}),
            is_read_only: true,
        },
        ToolDef {
            name: "get_chapter_outline",
            description: "获取指定章节的大纲（标题、摘要、情节点等）",
            parameters: json!({
                "type": "object",
                "properties": {
                    "chapter_number": {"type": "integer", "description": "章节号"}
                },
                "required": ["chapter_number"]
            }),
            is_read_only: true,
        },
        ToolDef {
            name: "get_plot_outline",
            description: "获取当前项目的情节大纲（幕和章节结构）",
            parameters: json!({"type": "object", "properties": {}, "required": []}),
            is_read_only: true,
        },
        ToolDef {
            name: "get_chapter",
            description: "获取指定章节的内容",
            parameters: json!({
                "type": "object",
                "properties": {
                    "chapter_number": {"type": "integer", "description": "章节号"}
                },
                "required": ["chapter_number"]
            }),
            is_read_only: true,
        },
        ToolDef {
            name: "get_chapter_summaries",
            description: "获取所有已生成的章节摘要（含关键事件、角色变化、伏笔等）",
            parameters: json!({"type": "object", "properties": {}, "required": []}),
            is_read_only: true,
        },
        ToolDef {
            name: "search_chapters",
            description: "在所有章节中搜索关键词",
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "搜索关键词"}
                },
                "required": ["query"]
            }),
            is_read_only: true,
        },
        ToolDef {
            name: "generate_world",
            description: "为当前项目生成世界观设定（地理、规则、势力、历史等）",
            parameters: json!({"type": "object", "properties": {}, "required": []}),
            is_read_only: false,
        },
        ToolDef {
            name: "generate_characters",
            description: "为当前项目生成角色设定（需要先有世界观）",
            parameters: json!({"type": "object", "properties": {}, "required": []}),
            is_read_only: false,
        },
        ToolDef {
            name: "generate_plot",
            description: "为当前项目生成情节大纲（需要先有世界观和角色）",
            parameters: json!({
                "type": "object",
                "properties": {
                    "target_chapters": {"type": "integer", "description": "目标章节数，默认50"}
                },
                "required": []
            }),
            is_read_only: false,
        },
        ToolDef {
            name: "expand_chapter",
            description: "扩写指定章节。工具内部会自动获取该章的大纲、前文上下文、角色状态和伏笔信息，无需先调用其他查询工具",
            parameters: json!({
                "type": "object",
                "properties": {
                    "chapter_number": {"type": "integer", "description": "章节号"},
                    "target_words": {"type": "integer", "description": "目标字数，默认3000"},
                    "hint": {"type": "string", "description": "额外写作要求或提示"}
                },
                "required": ["chapter_number"]
            }),
            is_read_only: false,
        },
        ToolDef {
            name: "continue_chapter",
            description: "续写指定章节。工具内部会自动获取上下文，无需先查询",
            parameters: json!({
                "type": "object",
                "properties": {
                    "chapter_number": {"type": "integer", "description": "章节号"},
                    "target_words": {"type": "integer", "description": "续写字数，默认1000"},
                    "instruction": {"type": "string", "description": "续写方向指示"}
                },
                "required": ["chapter_number"]
            }),
            is_read_only: false,
        },
        ToolDef {
            name: "review_chapter",
            description: "审校指定章节（检查逻辑、文笔、一致性等）",
            parameters: json!({
                "type": "object",
                "properties": {
                    "chapter_number": {"type": "integer", "description": "章节号"},
                    "platform": {"type": "string", "description": "目标平台（番茄/起点/纵横），默认番茄"}
                },
                "required": ["chapter_number"]
            }),
            is_read_only: false,
        },
        ToolDef {
            name: "export_novel",
            description: "导出整部小说为文件",
            parameters: json!({
                "type": "object",
                "properties": {
                    "format": {"type": "string", "description": "导出格式（txt/md/html），默认txt"}
                },
                "required": []
            }),
            is_read_only: false,
        },
    ]
}

#[allow(dead_code)]
pub fn tool_label(name: &str) -> &str {
    match name {
        "get_project_info" => "查看项目信息",
        "get_world" => "查看世界观",
        "get_characters" => "查看角色",
        "get_plot_outline" => "查看情节大纲",
        "get_chapter_outline" => "查看章节大纲",
        "get_chapter" => "查看章节",
        "get_chapter_summaries" => "查看章节摘要",
        "search_chapters" => "搜索章节",
        "generate_world" => "生成世界观",
        "generate_characters" => "生成角色",
        "generate_plot" => "生成情节大纲",
        "expand_chapter" => "扩写章节",
        "continue_chapter" => "续写章节",
        "review_chapter" => "审校章节",
        "export_novel" => "导出小说",
        _ => name,
    }
}

fn truncate(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

fn validate_tool_input(name: &str, params: &Value) -> Result<(), String> {
    match name {
        "get_chapter_outline" | "get_chapter" | "expand_chapter"
        | "continue_chapter" | "review_chapter" => {
            if let Some(v) = params.get("chapter_number") {
                if !v.is_u64() {
                    return Err(format!("chapter_number 必须是正整数，收到: {}", v));
                }
            }
            if matches!(name, "expand_chapter" | "continue_chapter") {
                if let Some(v) = params.get("target_words") {
                    if !v.is_u64() {
                        return Err(format!("target_words 必须是正整数，收到: {}", v));
                    }
                }
            }
        }
        "search_chapters" => {
            if params["query"].as_str().unwrap_or("").is_empty() {
                return Err("搜索关键词不能为空".into());
            }
        }
        "export_novel" => {
            if let Some(f) = params["format"].as_str() {
                if !matches!(f, "txt" | "md" | "html") {
                    return Err(format!("导出格式必须是 txt/md/html，收到: {}", f));
                }
            }
        }
        "generate_plot" => {
            if let Some(v) = params.get("target_chapters") {
                if !v.is_u64() {
                    return Err(format!("target_chapters 必须是正整数，收到: {}", v));
                }
            }
        }
        _ => {}
    }
    Ok(())
}

pub async fn execute_tool(
    name: &str,
    params: &Value,
    project_id: &str,
    client: &LlmClient,
    constraints_text: &str,
) -> Result<Value, String> {
    validate_tool_input(name, params)?;
    match name {
        "get_project_info" => {
            let meta = storage::load_json(project_id, "meta.json")?.unwrap_or(json!({}));
            Ok(meta)
        }
        "get_world" => {
            storage::load_json(project_id, "world.json")?
                .ok_or_else(|| "世界观尚未生成".into())
        }
        "get_characters" => {
            storage::load_json(project_id, "characters.json")?
                .ok_or_else(|| "角色尚未生成".into())
        }
        "get_plot_outline" => {
            storage::load_json(project_id, "plot.json")?
                .ok_or_else(|| "情节大纲尚未生成".into())
        }
        "get_chapter_outline" => {
            let num = params["chapter_number"].as_u64().unwrap_or(1) as u32;
            let outline = crate::find_chapter_outline(project_id, num)?;
            if outline.is_empty() {
                Err(format!("第{}章在大纲中未找到", num))
            } else {
                Ok(json!({"chapter_number": num, "outline": outline}))
            }
        }
        "get_chapter" => {
            let num = params["chapter_number"].as_u64().unwrap_or(1) as u32;
            let file = format!("chapter_{:03}.json", num);
            let ch = storage::load_json(project_id, &file)?
                .ok_or_else(|| format!("第{}章还没有内容", num))?;
            let text = ch["text"].as_str().unwrap_or("");
            Ok(json!({
                "chapter_number": num,
                "word_count": text.chars().count(),
                "text": truncate(text, 3000)
            }))
        }
        "get_chapter_summaries" => {
            storage::load_json(project_id, "chapter_summaries.json")?
                .ok_or_else(|| "还没有章节摘要".into())
        }
        "search_chapters" => {
            let query = params["query"].as_str().unwrap_or("");
            if query.is_empty() {
                return Err("搜索关键词不能为空".into());
            }
            let query_lower = query.to_lowercase();
            let mut results: Vec<Value> = Vec::new();
            for num in 1..=999u32 {
                if results.len() >= 20 { break; }
                let file = format!("chapter_{:03}.json", num);
                if let Ok(Some(ch)) = storage::load_json(project_id, &file) {
                    if let Some(text) = ch["text"].as_str() {
                        if text.to_lowercase().contains(&query_lower) {
                            let count = text.to_lowercase().matches(&query_lower).count();
                            results.push(json!({
                                "chapter": num,
                                "matches": count,
                                "preview": truncate(text, 200)
                            }));
                        }
                    }
                }
            }
            Ok(json!(results))
        }
        "generate_world" => {
            let meta = storage::load_json(project_id, "meta.json")?.ok_or("项目不存在")?;
            let outline = storage::load_json(project_id, "outline_source.json")?.unwrap_or(json!({}));
            let premise = meta["premise"].as_str().unwrap_or("");
            let genre = meta["genre"].as_str().unwrap_or("");
            let tone = meta["tone"].as_str().unwrap_or("");
            let themes: Vec<String> = meta["themes"].as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let outline_str = serde_json::to_string_pretty(&outline).unwrap_or_default();
            let world = engine::generate_world(
                client,
                &format!("{}\n\n{}\n\n## 已导入大纲\n{}", constraints_text, premise, truncate(&outline_str, 3000)),
                genre, tone, &themes,
            ).await?;
            storage::save_json(project_id, "world.json", &world)?;
            Ok(json!({"status": "success", "overview": world["overview"]}))
        }
        "generate_characters" => {
            let meta = storage::load_json(project_id, "meta.json")?.ok_or("项目不存在")?;
            let world = storage::load_json(project_id, "world.json")?.ok_or("请先生成世界观")?;
            let outline = storage::load_json(project_id, "outline_source.json")?.unwrap_or(json!({}));
            let premise = meta["premise"].as_str().unwrap_or("");
            let genre = meta["genre"].as_str().unwrap_or("");
            let tone = meta["tone"].as_str().unwrap_or("");
            let world_summary = serde_json::to_string(&json!({
                "era": world["era"], "overview": world["overview"], "factions": world["factions"]
            })).unwrap_or_default();
            let outline_str = serde_json::to_string_pretty(&outline).unwrap_or_default();
            let chars = engine::generate_characters(
                client,
                &format!("{}\n\n{}\n\n## 已导入大纲\n{}", constraints_text, premise, truncate(&outline_str, 3000)),
                genre, tone, &world_summary,
            ).await?;
            storage::save_json(project_id, "characters.json", &chars)?;
            let names: Vec<&str> = chars["characters"].as_array()
                .map(|a| a.iter().filter_map(|c| c["name"].as_str()).collect())
                .unwrap_or_default();
            Ok(json!({"status": "success", "characters": names}))
        }
        "generate_plot" => {
            let meta = storage::load_json(project_id, "meta.json")?.ok_or("项目不存在")?;
            let world = storage::load_json(project_id, "world.json")?.ok_or("请先生成世界观")?;
            let chars = storage::load_json(project_id, "characters.json")?.ok_or("请先生成角色")?;
            let premise = meta["premise"].as_str().unwrap_or("");
            let genre = meta["genre"].as_str().unwrap_or("");
            let tone = meta["tone"].as_str().unwrap_or("");
            let target = params["target_chapters"].as_u64().map(|v| v as u32);
            let world_summary = serde_json::to_string(&json!({
                "era": world["era"], "overview": world["overview"]
            })).unwrap_or_default();
            let chars_summary = serde_json::to_string(&chars["characters"]).unwrap_or_default();
            let plot = engine::generate_plot(
                client,
                &format!("{}\n\n{}", constraints_text, premise),
                genre, tone, &world_summary, truncate(&chars_summary, 3000), target,
            ).await?;
            storage::save_json(project_id, "plot.json", &plot)?;
            let ch_count = plot["acts"].as_array()
                .map(|a| a.iter().flat_map(|act| act["chapters"].as_array()).flatten().count())
                .unwrap_or(0);
            Ok(json!({"status": "success", "acts": plot["acts"].as_array().map(|a| a.len()).unwrap_or(0), "chapters": ch_count}))
        }
        "expand_chapter" => {
            let num = params["chapter_number"].as_u64().unwrap_or(1) as u32;
            let target_words = params["target_words"].as_u64().unwrap_or(3000) as u32;
            let hint = params["hint"].as_str().unwrap_or("");
            let chapter_outline = crate::find_chapter_outline(project_id, num)?;
            let mut context = crate::build_rich_context_string(project_id, num)?;
            eprintln!("[Tool/expand_chapter] ch={}, outline={}chars, context={}chars, constraints={}chars",
                num, chapter_outline.chars().count(), context.chars().count(), constraints_text.chars().count());
            if let Ok(Some(style)) = storage::load_json(project_id, "style_profile.json") {
                if let Some(summary) = style["summary"].as_str() {
                    context.push_str(&format!("\n\n## 作者文风\n{}", summary));
                }
            }
            let result = engine::expand_chapter(
                client,
                &format!("{}\n\n{}", truncate(constraints_text, 2000), chapter_outline),
                hint, target_words, &context,
            ).await?;
            let chapter_file = format!("chapter_{:03}.json", num);
            let lock = storage::project_lock(project_id);
            let _g = lock.lock().unwrap_or_else(|e| e.into_inner());
            if let Ok(Some(existing)) = storage::load_json(project_id, &chapter_file) {
                let old = existing["text"].as_str().unwrap_or("");
                if !old.is_empty() {
                    let _ = storage::save_snapshot(project_id, num, old);
                }
            }
            storage::save_json(project_id, &chapter_file, &result)?;
            let wc = result["text"].as_str().map(|t| t.chars().count()).unwrap_or(0);
            Ok(json!({"status": "success", "chapter": num, "word_count": wc}))
        }
        "continue_chapter" => {
            let num = params["chapter_number"].as_u64().unwrap_or(1) as u32;
            let target_words = params["target_words"].as_u64().unwrap_or(1000) as u32;
            let instruction = params["instruction"].as_str().unwrap_or("");
            let chapter_file = format!("chapter_{:03}.json", num);
            let existing = storage::load_json(project_id, &chapter_file)?
                .ok_or_else(|| format!("第{}章还没有内容，请先扩写", num))?;
            let existing_text = existing["text"].as_str().unwrap_or("");
            let mut context = crate::build_rich_context_string(project_id, num)?;
            if let Ok(Some(style)) = storage::load_json(project_id, "style_profile.json") {
                if let Some(summary) = style["summary"].as_str() {
                    context.push_str(&format!("\n\n## 作者文风\n{}", summary));
                }
            }
            let chars_count = existing_text.chars().count();
            let tail = if chars_count > 2000 {
                let skip = chars_count - 2000;
                &existing_text[existing_text.char_indices().nth(skip).map(|(i, _)| i).unwrap_or(0)..]
            } else {
                existing_text
            };
            let result = engine::continue_writing(
                client, tail,
                &format!("{}\n\n{}", truncate(constraints_text, 2000), instruction),
                target_words, &context,
            ).await?;
            // Persist under the project lock; re-read to avoid overwriting a change
            // made during generation, and snapshot so the append is recoverable.
            let lock = storage::project_lock(project_id);
            let _g = lock.lock().unwrap_or_else(|e| e.into_inner());
            let current = storage::load_json(project_id, &chapter_file)?
                .ok_or_else(|| format!("第{}章内容缺失", num))?;
            if current["text"].as_str().unwrap_or("") != existing_text {
                return Err(format!("第{}章在生成期间被修改，续写已取消", num));
            }
            if !existing_text.is_empty() {
                let _ = storage::save_snapshot(project_id, num, existing_text);
            }
            let new_text = format!("{}\n\n{}", existing_text, result["text"].as_str().unwrap_or(""));
            let updated = json!({"text": new_text});
            storage::save_json(project_id, &chapter_file, &updated)?;
            Ok(json!({"status": "success", "chapter": num, "word_count": new_text.chars().count()}))
        }
        "review_chapter" => {
            let num = params["chapter_number"].as_u64().unwrap_or(1) as u32;
            let platform = params["platform"].as_str().unwrap_or("番茄");
            let chapter_file = format!("chapter_{:03}.json", num);
            let ch = storage::load_json(project_id, &chapter_file)?
                .ok_or_else(|| format!("第{}章还没有内容", num))?;
            let text = ch["text"].as_str().unwrap_or("");
            if text.is_empty() {
                return Err(format!("第{}章内容为空，无法审校", num));
            }
            let meta = storage::load_json(project_id, "meta.json")?.unwrap_or(json!({}));
            let genre = meta["genre"].as_str().unwrap_or("未知");
            let chapter_truncated = truncate(text, 8000);
            let system = "你是一位拥有12年+网文行业从业经验的资深编辑。请对提供的章节进行全维度检查。";
            let user_prompt = format!(
                "{}\n\n请对以下第{}章内容进行专业审校。\n小说类型：{}\n目标平台：{}\n\n## 章节内容\n{}\n\n请检查：逻辑、人设一致性、剧情节奏、文字规范、平台合规性、优化建议。",
                constraints_text, num, genre, platform, chapter_truncated
            );
            let msgs = vec![("user".to_string(), user_prompt)];
            let review = client.chat(system, &msgs, 4096).await?;
            Ok(json!({"chapter": num, "review": review}))
        }
        "export_novel" => {
            let format = params["format"].as_str().unwrap_or("txt");
            let meta = storage::load_json(project_id, "meta.json")?.ok_or("项目不存在")?;
            let title = meta["title"].as_str().unwrap_or("novel");
            let plot = storage::load_json(project_id, "plot.json")?;
            let mut chapter_nums: Vec<u32> = Vec::new();
            if let Some(p) = &plot {
                if let Some(acts) = p["acts"].as_array() {
                    for act in acts {
                        if let Some(chapters) = act["chapters"].as_array() {
                            for ch in chapters {
                                if let Some(n) = ch["number"].as_u64() {
                                    chapter_nums.push(n as u32);
                                }
                            }
                        }
                    }
                }
            }
            if chapter_nums.is_empty() {
                for n in 1..=999u32 {
                    let file = format!("chapter_{:03}.json", n);
                    if storage::load_json(project_id, &file).ok().flatten().is_some() {
                        chapter_nums.push(n);
                    } else {
                        break;
                    }
                }
            }
            let mut content = format!("《{}》\n\n", title);
            for num in &chapter_nums {
                let file = format!("chapter_{:03}.json", num);
                if let Ok(Some(ch)) = storage::load_json(project_id, &file) {
                    if let Some(text) = ch["text"].as_str() {
                        content.push_str(&format!("第{}章\n\n{}\n\n", num, text));
                    }
                }
            }
            let export_dir = dirs::download_dir()
                .or_else(dirs::document_dir)
                .or_else(dirs::home_dir)
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            let safe_title: String = title.chars()
                .filter(|c| !c.is_control() && !matches!(*c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
                .take(50).collect();
            let path = export_dir.join(format!("{}.{}", safe_title, format));
            std::fs::write(&path, &content).map_err(|e| format!("写入失败: {}", e))?;
            Ok(json!({"status": "success", "path": path.to_string_lossy()}))
        }
        _ => Err(format!("未知工具: {}", name)),
    }
}
