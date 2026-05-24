//! Context pruning for provider requests.

use crate::memory::HistoryMessage;
use crate::providers::Message;

const DEFAULT_RECENT_TURNS: usize = 10;
const MAX_SUMMARY_CHARS: usize = 1_200;

pub fn clamp_recent_turns(value: usize) -> usize {
    if value == 0 {
        DEFAULT_RECENT_TURNS
    } else {
        value.clamp(1, 50)
    }
}

pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4).max(1)
}

pub fn truncate_for_summary(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    let keep_each_side = max_chars.saturating_sub(80) / 2;
    let head: String = text.chars().take(keep_each_side).collect();
    let tail: String = text
        .chars()
        .rev()
        .take(keep_each_side)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!(
        "{}\n...[truncated {} chars for context budget]...\n{}",
        head,
        text.chars().count().saturating_sub(keep_each_side * 2),
        tail
    )
}

pub fn summarize_older_history(history: &[HistoryMessage]) -> Option<String> {
    if history.is_empty() {
        return None;
    }

    let mut lines = Vec::new();
    let total = history.len();
    let first = total.saturating_sub(20);
    for item in history.iter().skip(first) {
        let content = truncate_for_summary(&item.content, 220).replace('\n', " ");
        lines.push(format!("- {}: {}", item.role, content));
    }

    Some(format!(
        "Earlier conversation summary, compressed by AUXLOCLAW to save context tokens. Original older message count: {}. Most recent older items:\n{}",
        total,
        lines.join("\n")
    ))
}

pub fn build_pruned_messages(
    system_prompt: String,
    history: &[HistoryMessage],
    user_message: String,
    recent_turns: usize,
    context_window_tokens: u32,
) -> Vec<Message> {
    let recent_turns = clamp_recent_turns(recent_turns);
    let keep_messages = recent_turns.saturating_mul(2);
    let split_at = history.len().saturating_sub(keep_messages);
    let (older, recent) = history.split_at(split_at);

    let mut sys_prompt = system_prompt;
    if let Some(summary) = summarize_older_history(older) {
        sys_prompt.push_str("\n\n## Earlier Conversation Summary\n");
        sys_prompt.push_str(&summary);
    }

    let mut messages = vec![Message::new("system", sys_prompt)];

    for m in recent {
        if m.role == "system" {
            if let Some(ref mut first) = messages.first_mut() {
                if let Some(ref mut content) = first.content {
                    content.push_str("\n\n");
                    content.push_str(&m.content);
                }
            }
        } else {
            messages.push(Message::new(
                &m.role,
                truncate_for_summary(&m.content, MAX_SUMMARY_CHARS),
            ));
        }
    }

    messages.push(Message::new("user", user_message));

    trim_to_token_budget(messages, context_window_tokens as usize)
}

fn trim_to_token_budget(mut messages: Vec<Message>, max_tokens: usize) -> Vec<Message> {
    if max_tokens == 0 {
        return messages;
    }

    while estimate_message_tokens(&messages) > max_tokens && messages.len() > 2 {
        let remove_index = messages
            .iter()
            .position(|m| m.role != "system")
            .unwrap_or(1);
        messages.remove(remove_index);
    }

    messages
}

fn estimate_message_tokens(messages: &[Message]) -> usize {
    messages
        .iter()
        .map(|m| {
            estimate_tokens(&m.role)
                + estimate_tokens(m.content.as_deref().unwrap_or(""))
                + 4
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history(role: &str, content: &str) -> HistoryMessage {
        HistoryMessage {
            role: role.into(),
            content: content.into(),
            timestamp: 0,
            tool_calls: None,
        }
    }

    #[test]
    fn keeps_recent_ten_turns_by_default() {
        let mut h = Vec::new();
        for i in 0..30 {
            h.push(history("user", &format!("message {}", i)));
        }
        let messages = build_pruned_messages("sys".into(), &h, "now".into(), 10, 50_000);
        let joined = messages
            .iter()
            .map(|m| m.content.as_deref().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("Original older message count: 10"));
        assert!(joined.contains("message 10"));
        assert!(joined.contains("message 29"));
        assert!(joined.contains("Earlier conversation summary"));
    }

    #[test]
    fn clamps_recent_turns() {
        assert_eq!(clamp_recent_turns(0), 10);
        assert_eq!(clamp_recent_turns(8), 8);
        assert_eq!(clamp_recent_turns(100), 50);
    }

    #[test]
    fn merges_system_messages_from_compaction() {
        let mut h = Vec::new();
        h.push(history("system", "[COMPACTION SUMMARY] Prior context was about building a proxy."));
        h.push(history("user", "what was I working on?"));
        let messages = build_pruned_messages("You are an agent.".into(), &h, "tell me".into(), 10, 50_000);
        let system_msgs: Vec<_> = messages.iter().filter(|m| m.role == "system").collect();
        assert_eq!(system_msgs.len(), 1, "must have exactly one system message");
        let sys_content = system_msgs[0].content.as_deref().unwrap_or("");
        assert!(sys_content.contains("You are an agent."));
        assert!(sys_content.contains("COMPACTION SUMMARY"));
        assert!(sys_content.contains("Prior context"));
    }
}
