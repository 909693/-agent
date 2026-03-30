pub fn world_building(premise: &str, genre: &str, tone: &str, themes: &[String]) -> String {
    let themes_str = if themes.is_empty() {
        "未指定".to_string()
    } else {
        themes.join("、")
    };
    format!(
        r#"请为以下小说构建完整的世界设定。

## 故事前提
{premise}

## 类型：{genre}
## 基调：{tone}
## 核心主题：{themes_str}

请设计：
1. era: 时代背景
2. overview: 世界概述（2-3段）
3. geography: 3-6个关键地点（name, description, significance, connected_to）
4. rules: 2-5条世界规则（category, name, description, limitations, plot_implications）
5. factions: 2-4个势力（name, description, goals, key_members）
6. history: 3-5个关键历史事件
7. culture_notes: 2-4条文化特征

输出 JSON 格式：
{{"era":"","overview":"","geography":[{{"name":"","description":"","significance":"","connected_to":[]}}],"rules":[{{"category":"","name":"","description":"","limitations":[],"plot_implications":[]}}],"factions":[{{"name":"","description":"","goals":[],"key_members":[]}}],"history":[],"culture_notes":[]}}"#
    )
}

pub fn character_gen(premise: &str, genre: &str, tone: &str, world_summary: &str) -> String {
    format!(
        r#"请为以下小说设计角色群像。

## 故事前提
{premise}

## 类型：{genre} / 基调：{tone}

## 世界设定摘要
{world_summary}

请设计 5-8 个角色，包含：
- 1个主角（protagonist）
- 1个反派（antagonist）
- 3-6个配角（supporting）

每个角色需要：id（用 UUID）、name、role、age、appearance、personality、backstory、motivations、secrets、skills、arc（start_state, end_state, key_turning_points, internal_conflict）、relationships（target, rel_type, description, evolution）、faction

输出 JSON：
{{"characters":[{{"id":"","name":"","role":"","age":"","appearance":"","personality":"","backstory":"","motivations":[],"secrets":[],"skills":[],"arc":{{"start_state":"","end_state":"","key_turning_points":[],"internal_conflict":""}},"relationships":[{{"target":"","rel_type":"","description":"","evolution":""}}],"faction":""}}]}}"#
    )
}

pub fn plot_outline(
    premise: &str,
    genre: &str,
    tone: &str,
    world_summary: &str,
    characters_summary: &str,
) -> String {
    format!(
        r#"请为以下小说设计情节大纲。

## 故事前提
{premise}

## 类型：{genre} / 基调：{tone}

## 世界设定
{world_summary}

## 角色
{characters_summary}

请设计 3-5 幕（Act），每幕包含 5-10 章。同时列出关键情节点和伏笔。

输出 JSON：
{{"acts":[{{"number":1,"title":"","theme":"","chapters":[{{"number":1,"title":"","summary":"","pov_character":"","plot_points":[],"location":""}}]}}],"plot_points":[{{"id":"","type":"","summary":"","characters_involved":[],"location":"","foreshadowing":[],"consequences":[]}}],"subplots":[]}}"#
    )
}

pub fn timeline_gen(plot_summary: &str, world_summary: &str) -> String {
    format!(
        r#"请根据情节大纲生成时间线。

## 情节大纲
{plot_summary}

## 世界设定
{world_summary}

为每个关键事件生成时间线条目，按时间顺序排列。

输出 JSON：
{{"events":[{{"id":"","timestamp":"","sort_key":0,"description":"","characters_involved":[],"location":"","chapter_ref":0}}]}}"#
    )
}

pub fn chapter_expand(
    chapter_outline: &str,
    user_content: &str,
    target_words: u32,
    context: &str,
) -> String {
    let user_part = if user_content.is_empty() {
        "（用户未提供初始内容，请根据大纲从头创作）".to_string()
    } else {
        format!("## 用户已写内容\n{user_content}")
    };
    format!(
        r#"请根据以下信息扩写本章内容，目标字数约 {target_words} 字。

## 章节大纲
{chapter_outline}

{user_part}

## 上下文（世界观、角色、前情）
{context}

要求：
1. 如果用户已写内容，在其基础上扩展和丰富，保持用户的文风和叙事方向
2. 如果用户未写内容，根据大纲创作完整章节
3. 保持与上下文的一致性
4. 目标字数约 {target_words} 字

输出 JSON：{{"text": "扩写后的完整章节正文"}}"#
    )
}

pub fn continue_write(
    existing_text: &str,
    instruction: &str,
    target_words: u32,
    context: &str,
) -> String {
    let last_500: String = existing_text
        .chars()
        .rev()
        .take(500)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!(
        r#"请续写以下小说内容，目标续写约 {target_words} 字。

## 已有文本（末尾部分）
...{last_500}

## 续写指示
{instruction}

## 上下文
{context}

要求：
1. 无缝衔接已有文本，保持文风一致
2. 按照续写指示推进情节
3. 目标续写约 {target_words} 字

输出 JSON：{{"text": "续写的内容"}}"#
    )
}
