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
    target_chapters: Option<u32>,
) -> String {
    // Dynamically compute act structure based on target chapters
    let target_chapters = target_chapters.unwrap_or(50);
    let (min_acts, max_acts, chs_per_act_hint) = if target_chapters <= 20 {
        (3, 4, format!("每幕约 {}-{} 章", target_chapters / 4, target_chapters / 3))
    } else if target_chapters <= 60 {
        (4, 6, format!("每幕约 {}-{} 章", target_chapters / 6, target_chapters / 4))
    } else if target_chapters <= 150 {
        (5, 8, format!("每幕约 {}-{} 章", target_chapters / 8, target_chapters / 5))
    } else if target_chapters <= 300 {
        (8, 12, format!("每幕约 {}-{} 章", target_chapters / 12, target_chapters / 8))
    } else {
        (10, 15, format!("每幕约 {}-{} 章", target_chapters / 15, target_chapters / 10))
    };

    format!(
        r#"请为以下小说设计情节大纲。

## 故事前提
{premise}

## 类型：{genre} / 基调：{tone}

## 世界设定
{world_summary}

## 角色
{characters_summary}

## 规模要求
- 目标总章数：约 {target_chapters} 章
- 幕数：{min_acts}-{max_acts} 幕
- {chs_per_act_hint}
- 务必确保所有章节编号从 1 开始连续递增到 {target_chapters}

请设计 {min_acts}-{max_acts} 幕（Act），总计约 {target_chapters} 章。同时列出关键情节点和伏笔。

输出 JSON：
{{"acts":[{{"number":1,"title":"","theme":"","chapters":[{{"number":1,"title":"","summary":"","pov_character":"","plot_points":[],"location":""}}]}}],"plot_points":[{{"id":"","type":"","summary":"","characters_involved":[],"location":"","foreshadowing":[],"consequences":[]}}],"subplots":[]}}"#
    )
}

pub fn plot_skeleton(
    premise: &str,
    genre: &str,
    tone: &str,
    world_summary: &str,
    characters_summary: &str,
    target_chapters: u32,
) -> String {
    let (min_acts, max_acts, chs_per_act_hint) = if target_chapters <= 150 {
        (5, 8, format!("每幕约 {}-{} 章", target_chapters / 8, target_chapters / 5))
    } else if target_chapters <= 300 {
        (8, 12, format!("每幕约 {}-{} 章", target_chapters / 12, target_chapters / 8))
    } else {
        (10, 15, format!("每幕约 {}-{} 章", target_chapters / 15, target_chapters / 10))
    };

    format!(
        r#"请为以下小说设计故事骨架（仅幕级结构，不要输出单独章节详情）。

## 故事前提
{premise}

## 类型：{genre} / 基调：{tone}

## 世界设定
{world_summary}

## 角色
{characters_summary}

## 规模要求
- 目标总章数：约 {target_chapters} 章
- 幕数：{min_acts}-{max_acts} 幕
- {chs_per_act_hint}

请设计 {min_acts}-{max_acts} 幕（Act）。每幕给出标题、主题、关键事件、结尾状态。
**重要约束**：
- 你生成的最后一幕（数组最后一个元素）必须是全书的**终局/大结局**，涵盖最终决战、核心冲突的彻底解决、所有伏笔的回收和角色的最终命运走向。
- 倒数第二幕应是终局前的**高潮与决战前夕**。
- 关键情节点必须按时间顺序合理分布在各幕中。
注意：不要输出章节范围（chapter_start/chapter_end），代码会自动计算精确的章节分配。
同时列出全局关键情节点和子线。

输出 JSON：
{{"acts":[{{"number":1,"title":"","theme":"","key_events":["关键事件简述"],"characters_focus":["角色名"],"end_state":"该幕结尾状态（50字以内）"}}],"plot_points":[{{"id":"","type":"","summary":"","characters_involved":[],"location":"","foreshadowing":[],"consequences":[]}}],"subplots":[]}}"#
    )
}

pub fn act_chapter_details(
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
) -> String {
    let prev_part = if prev_act_summary.is_empty() {
        "（第一幕，无前情）".to_string()
    } else {
        format!("## 前一幕\n{prev_act_summary}")
    };
    let next_part = if next_act_summary.is_empty() {
        "（这是全书的最后一幕/终局！必须包含：核心冲突的彻底解决、所有主要角色的最终命运、所有伏笔的回收。必须是有力的、令人印象深刻的结局。）".to_string()
    } else {
        format!("## 后一幕\n{next_act_summary}")
    };

    format!(
        r#"请为小说第 {act_number} 幕生成详细章节大纲。

## 本幕信息
- 标题：{act_title}
- 主题：{act_theme}
- 关键事件：{act_key_events}
- 结尾状态：{act_end_state}
- 章节范围：第 {chapter_start} 章到第 {chapter_end} 章

{prev_part}

{next_part}

## 故事背景
{story_context}

## 全局情节点
{plot_points_json}

请为第 {chapter_start} 到第 {chapter_end} 章生成详细大纲。每章必须有独特标题和 100-200 字的摘要。
确保情节围绕本幕主题「{act_theme}」展开，覆盖所有关键事件。
章节编号必须从 {chapter_start} 连续到 {chapter_end}。

输出 JSON：
{{"chapters":[{{"number":{chapter_start},"title":"","summary":"","pov_character":"","plot_points":[],"location":""}}]}}"#
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

## 章节大纲与创作规则
{chapter_outline}

⚠️ 以上"创作规则"部分（如果有 Skills 规则或提示词要求）是用户设定的强制约束，你必须在创作中严格遵循。

{user_part}

## 上下文（世界观、角色、前情）
{context}

要求：
1. 严格遵循上方的 Skills 规则和提示词要求（如果有的话）
2. 如果用户已写内容，在其基础上扩展和丰富，保持用户的文风和叙事方向
3. 如果用户未写内容，根据大纲创作完整章节
4. 保持与上下文的一致性
5. 目标字数约 {target_words} 字

直接输出章节正文，不要包裹 JSON 或 markdown 代码块。"#
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

直接输出续写的正文，不要包裹 JSON 或 markdown 代码块。"#
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

pub fn sync_outline(chapter_text: &str, chapter_number: u32) -> String {
    let truncated: String = if chapter_text.chars().count() > 5000 { chapter_text.chars().take(5000).collect::<String>() } else { chapter_text.to_string() };
    format!(
        r#"请根据以下第{chapter_number}章的正文内容，生成准确的章节摘要。

## 章节正文
{truncated}

输出 JSON：
{{
  "summary": "200字以内的章节剧情摘要，准确反映正文实际内容"
}}"#
    )
}

pub fn name_generator(world_summary: &str, name_type: &str, count: u32) -> String {
    let type_label = match name_type {
        "character" => "人物名字",
        "place" => "地名",
        "skill" => "功法/技能名",
        "item" => "物品/道具名",
        _ => "名字",
    };
    format!(
        r#"请根据以下世界设定，生成 {count} 个{type_label}。

## 世界设定
{world_summary}

要求：
1. 名字须符合世界观的时代背景和文化氛围
2. 避免与常见网文重复的俗套命名
3. 每个名字附带简短来由说明

输出 JSON：
{{
  "names": [
    {{"name": "名字", "origin": "命名来由/含义"}}
  ]
}}"#
    )
}

pub fn sensitivity_check(chapter_text: &str) -> String {
    let truncated: String = if chapter_text.chars().count() > 8000 { chapter_text.chars().take(8000).collect::<String>() } else { chapter_text.to_string() };
    format!(
        r#"请审核以下小说章节内容，检测可能触发网文平台审核的敏感内容。

## 章节正文
{truncated}

检测维度：
1. 政治敏感：涉政、影射现实政治人物或事件
2. 暴力血腥：过度详细的暴力描写
3. 色情擦边：性暗示、过度暴露描写
4. 违规用语：脏话、歧视性语言
5. 其他风险：迷信宣扬、毒品美化等

输出 JSON：
{{
  "risk_level": "safe/low/medium/high",
  "issues": [
    {{
      "location": "约第N段",
      "content": "问题片段（原文引用）",
      "category": "political/violence/sexual/profanity/other",
      "risk_level": "low/medium/high",
      "suggestion": "修改建议"
    }}
  ],
  "summary": "总体审核结论"
}}"#
    )
}
