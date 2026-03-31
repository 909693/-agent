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

pub fn consistency_check(
    summaries_json: &str,
    world_summary: &str,
    characters_summary: &str,
) -> String {
    format!(
        r#"请分析以下小说的全部章节摘要，检查前后一致性问题。

## 世界设定
{world_summary}

## 角色设定
{characters_summary}

## 各章摘要与状态
{summaries_json}

请严格检查以下维度：
1. 角色行为一致性：角色言行是否与性格人设冲突，是否出现 OOC
2. 时间线一致性：事件先后顺序是否矛盾，时间跨度是否合理
3. 设定一致性：世界规则、地理、道具、能力等是否前后矛盾
4. 伏笔一致性：已埋伏笔是否被遗忘，回收是否与埋设矛盾
5. 情节逻辑：是否存在逻辑漏洞、不合理的巧合、未解释的转折

输出 JSON：
{{
  "issues": [
    {{
      "severity": "high/medium/low",
      "category": "character/timeline/setting/foreshadowing/plot_hole",
      "location": "第X章 vs 第Y章",
      "description": "具体矛盾描述",
      "suggestion": "修复建议"
    }}
  ],
  "overall_score": 85,
  "summary": "总体一致性评估（2-3句话）"
}}"#
    )
}

pub fn reader_simulation(
    chapter_text: &str,
    chapter_outline: &str,
    context: &str,
) -> String {
    format!(
        r#"请模拟三类读者阅读以下章节，给出真实反馈。

三类读者：
1. 追更老书虫：熟悉套路，关注爽点和节奏
2. 新入坑路人：首次接触，关注能否看懂和代入
3. 严苛挑刺党：关注逻辑、文笔和原创性

## 章节大纲
{chapter_outline}

## 前文背景
{context}

## 章节正文
{chapter_text}

请综合三类读者视角，输出 JSON：
{{
  "engagement_score": 75,
  "hook_power": "章首钩子是否有力，能否吸引继续阅读",
  "pacing_feel": "节奏感受：拖沓/紧凑/失控/张弛有度",
  "confusion_points": [
    {{"location": "约第N段", "issue": "不清楚为什么..."}}
  ],
  "excitement_peaks": [
    {{"location": "约第N段", "reaction": "这里很爽因为..."}}
  ],
  "drop_risks": [
    {{"location": "约第N段", "reason": "这里可能弃文因为..."}}
  ],
  "overall_feel": "一句话总结阅读感受"
}}"#
    )
}

pub fn analyze_style(samples: &str) -> String {
    format!(
        r#"请分析以下小说文本样本的写作风格特征。

## 文本样本
{samples}

请从以下维度提取风格特征，输出 JSON：
{{
  "narrative_voice": "叙述视角偏好（如第三人称限制视角、全知视角等）",
  "sentence_style": "句式特征（长短句比例、平均句长、节奏感）",
  "rhetoric_preference": ["常用修辞手法1", "修辞手法2"],
  "dialogue_style": "对话风格（简洁/繁复，'说'字的使用习惯）",
  "description_level": "描写详略偏好（动作/环境/心理/外貌各维度）",
  "pacing_pattern": "叙事节奏模式（快/慢/交替/渐进）",
  "vocabulary_tendency": "用词倾向（口语化/文雅/专业术语/方言等）",
  "emotional_tone": "情感基调（冷峻/温暖/幽默/热血等）",
  "summary": "用2-3句话总结该作者的核心文风特点，可直接用于指导AI写作"
}}"#
    )
}
