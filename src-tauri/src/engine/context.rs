use crate::llm::client::AgentMsg;

pub fn estimate_tokens(text: &str) -> usize {
    let mut cjk = 0usize;
    let mut ascii = 0usize;
    for c in text.chars() {
        if is_cjk(c) {
            cjk += 1;
        } else {
            ascii += 1;
        }
    }
    cjk * 2 + ascii / 4 + 1
}

fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}' |
        '\u{3400}'..='\u{4DBF}' |
        '\u{F900}'..='\u{FAFF}' |
        '\u{2E80}'..='\u{2EFF}' |
        '\u{3000}'..='\u{303F}' |
        '\u{FF00}'..='\u{FFEF}'
    )
}

fn msg_tokens(msg: &AgentMsg) -> usize {
    match msg {
        AgentMsg::User { content } => estimate_tokens(content) + 4,
        AgentMsg::Assistant { text, tool_uses } => {
            let mut t = estimate_tokens(text) + 4;
            for tu in tool_uses {
                t += estimate_tokens(&tu.name) + estimate_tokens(&serde_json::to_string(&tu.input).unwrap_or_default()) + 20;
            }
            t
        }
        AgentMsg::ToolResultMsg { content, .. } => estimate_tokens(content) + 10,
    }
}

fn truncate_content(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max_chars).collect();
    format!("{}...(已截断)", truncated)
}

/// Move a cut index forward so the "recent" slice never begins with a
/// ToolResultMsg. Otherwise its matching tool_use ends up in the summarized/
/// dropped part, leaving an orphan tool_result that makes Anthropic/OpenAI
/// reject the next request with a 400.
fn adjust_cut_past_tool_results(messages: &[AgentMsg], mut cut: usize) -> usize {
    while cut < messages.len() {
        if matches!(messages[cut], AgentMsg::ToolResultMsg { .. }) {
            cut += 1;
        } else {
            break;
        }
    }
    cut
}

pub fn compact_conversation(messages: &[AgentMsg], max_tokens: usize) -> Vec<AgentMsg> {
    let total: usize = messages.iter().map(|m| msg_tokens(m)).sum();
    if total <= max_tokens {
        return messages.to_vec();
    }

    let n = messages.len();
    let keep_recent = 6.min(n);
    let old_count = n.saturating_sub(keep_recent);
    // Don't let the recent slice start with an orphan tool_result.
    let old_count = adjust_cut_past_tool_results(messages, old_count);

    if old_count == 0 {
        return messages.to_vec();
    }

    let mut result: Vec<AgentMsg> = Vec::new();

    // Phase 1: Truncate tool results in old messages
    let mut old_msgs: Vec<AgentMsg> = messages[..old_count].to_vec();
    for msg in &mut old_msgs {
        if let AgentMsg::ToolResultMsg { content, .. } = msg {
            if content.chars().count() > 200 {
                *content = truncate_content(content, 200);
            }
        }
    }

    let old_tokens: usize = old_msgs.iter().map(|m| msg_tokens(m)).sum();
    let recent_tokens: usize = messages[old_count..].iter().map(|m| msg_tokens(m)).sum();

    if old_tokens + recent_tokens <= max_tokens {
        result.extend(old_msgs);
        result.extend_from_slice(&messages[old_count..]);
        return result;
    }

    // Phase 2: Summarize old messages into a single user message
    let mut summary_parts: Vec<String> = Vec::new();
    for msg in &old_msgs {
        match msg {
            AgentMsg::User { content } => {
                let preview: String = content.chars().take(100).collect();
                summary_parts.push(format!("用户: {}", preview));
            }
            AgentMsg::Assistant { text, tool_uses } => {
                let preview: String = text.chars().take(100).collect();
                if !tool_uses.is_empty() {
                    let tools: Vec<&str> = tool_uses.iter().map(|t| t.name.as_str()).collect();
                    summary_parts.push(format!("助手: {} [调用工具: {}]", preview, tools.join(", ")));
                } else if !text.is_empty() {
                    summary_parts.push(format!("助手: {}", preview));
                }
            }
            AgentMsg::ToolResultMsg { tool_use_id: _, content } => {
                let preview: String = content.chars().take(50).collect();
                summary_parts.push(format!("工具结果: {}", preview));
            }
        }
    }

    let summary = format!("[以下是之前对话的摘要]\n{}", summary_parts.join("\n"));
    result.push(AgentMsg::User { content: summary });
    result.extend_from_slice(&messages[old_count..]);
    result
}

pub fn aggressive_compact(messages: &[AgentMsg], max_tokens: usize) -> Vec<AgentMsg> {
    let half = compact_conversation(messages, max_tokens / 2);
    let total: usize = half.iter().map(|m| msg_tokens(m)).sum();
    if total <= max_tokens {
        return half;
    }

    let n = messages.len();
    let keep = 4.min(n);
    // Start the kept tail at a non-tool-result so we never orphan a tool_result.
    let cut = adjust_cut_past_tool_results(messages, n.saturating_sub(keep));
    let mut result: Vec<AgentMsg> = Vec::new();

    if cut > 0 {
        result.push(AgentMsg::User {
            content: "[之前的对话已被压缩以适应上下文限制]".to_string(),
        });
    }

    for msg in &messages[cut..] {
        match msg {
            AgentMsg::ToolResultMsg { tool_use_id, content } => {
                result.push(AgentMsg::ToolResultMsg {
                    tool_use_id: tool_use_id.clone(),
                    content: truncate_content(content, 100),
                });
            }
            other => result.push(other.clone()),
        }
    }
    result
}
