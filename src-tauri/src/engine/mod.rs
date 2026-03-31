pub mod prompts;

use crate::llm::client::LlmClient;
use serde_json::Value;

pub async fn generate_world(
    client: &LlmClient,
    premise: &str,
    genre: &str,
    tone: &str,
    themes: &[String],
) -> Result<Value, String> {
    let system = "你是一位专业的小说世界构建师。请始终以有效的 JSON 格式输出。";
    let user = prompts::world_building(premise, genre, tone, themes);
    client.generate_json(system, &user, 8192).await
}

pub async fn generate_characters(
    client: &LlmClient,
    premise: &str,
    genre: &str,
    tone: &str,
    world_summary: &str,
) -> Result<Value, String> {
    let system = "你是一位专业的小说角色设计师。请始终以有效的 JSON 格式输出。";
    let user = prompts::character_gen(premise, genre, tone, world_summary);
    client.generate_json(system, &user, 8192).await
}

pub async fn generate_plot(
    client: &LlmClient,
    premise: &str,
    genre: &str,
    tone: &str,
    world_summary: &str,
    characters_summary: &str,
) -> Result<Value, String> {
    let system = "你是一位专业的小说情节架构师。请始终以有效的 JSON 格式输出。";
    let user = prompts::plot_outline(premise, genre, tone, world_summary, characters_summary);
    client.generate_json(system, &user, 8192).await
}

pub async fn generate_timeline(
    client: &LlmClient,
    plot_summary: &str,
    world_summary: &str,
) -> Result<Value, String> {
    let system = "你是一位专业的小说时间线规划师。请始终以有效的 JSON 格式输出。";
    let user = prompts::timeline_gen(plot_summary, world_summary);
    client.generate_json(system, &user, 4096).await
}

pub async fn expand_chapter(
    client: &LlmClient,
    chapter_outline: &str,
    user_content: &str,
    target_words: u32,
    context: &str,
) -> Result<Value, String> {
    let system = "你是一位专业的小说作家。你擅长根据大纲和已有内容进行扩写，保持文风一致、情节连贯。直接输出小说正文，不要输出 JSON。";
    let user = prompts::chapter_expand(chapter_outline, user_content, target_words, context);
    client.generate_json(system, &user, 16384).await
}

pub async fn continue_writing(
    client: &LlmClient,
    existing_text: &str,
    instruction: &str,
    target_words: u32,
    context: &str,
) -> Result<Value, String> {
    let system = "你是一位专业的小说作家。请根据已有文本和指示继续创作，保持文风一致。直接输出续写内容，不要输出 JSON。";
    let user = prompts::continue_write(existing_text, instruction, target_words, context);
    client.generate_json(system, &user, 16384).await
}

/// Summarize a chapter for RAG context retrieval
pub async fn summarize_chapter(
    client: &LlmClient,
    chapter_number: u32,
    chapter_text: &str,
    chapter_outline: &str,
) -> Result<Value, String> {
    let system = "你是小说分析专家。请精确提取章节的关键信息，以 JSON 格式输出。";
    let user = format!(
        r#"请分析以下第{chapter_number}章内容，提取关键信息。

## 章节大纲
{chapter_outline}

## 章节正文（前3000字）
{}

请输出 JSON：
{{
  "chapter": {chapter_number},
  "summary": "100字以内的章节剧情摘要",
  "key_events": ["关键事件1", "关键事件2"],
  "characters_appeared": ["出场角色名1", "角色名2"],
  "character_changes": [
    {{"name": "角色名", "change": "状态变化描述（位置/伤势/情绪/实力等）"}}
  ],
  "foreshadowing_planted": ["本章埋下的伏笔"],
  "foreshadowing_resolved": ["本章回收的伏笔"],
  "settings_introduced": ["新出现的设定/地点/物品"],
  "end_state": "章节结尾时的场景状态，50字以内"
}}"#,
        &chapter_text[..chapter_text.len().min(3000)]
    );
    client.generate_json(system, &user, 2048).await
}

/// Check consistency across all chapters
pub async fn check_consistency(
    client: &LlmClient,
    summaries_json: &str,
    world_summary: &str,
    characters_summary: &str,
) -> Result<Value, String> {
    let system = "你是资深小说编辑，擅长发现长篇小说中的前后矛盾和逻辑漏洞。请以 JSON 格式输出分析结果。";
    let user = prompts::consistency_check(summaries_json, world_summary, characters_summary);
    client.generate_json(system, &user, 4096).await
}
