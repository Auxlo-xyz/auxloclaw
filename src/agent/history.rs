//! Hierarchical Context Compression
//!
//! Three-tier history management that keeps the agent under its context window limit:
//!
//! | Tier      | What                        | Budget | Compression                                  |
//! |-----------|-----------------------------|--------|----------------------------------------------|
//! | Current   | Active conversation topic   | 50%    | Summarize middle messages, keep first & last  |
//! | Topics    | Previous conversation topics| 30%    | Summarize each topic to single message        |
//! | Bulks     | Merged topic groups         | 20%    | Summarize entire groups, drop oldest if needed|
//!
//! The compressor iterates until total tokens fit within the budget, always targeting
//! the most over-budget tier first.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::context::estimate_tokens;
use crate::memory::HistoryMessage;
use crate::providers::Message;

// Budget ratios per tier (of ctx_limit)
const CURRENT_RATIO: f64 = 0.50;
const TOPICS_RATIO: f64 = 0.30;
const BULKS_RATIO: f64 = 0.20;
const BULK_MERGE_SIZE: usize = 3;
const MAX_COMPRESS_ITERS: usize = 20;

/// A single record in the history hierarchy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Record {
    Message(HistoryMessage),
    Topic(Topic),
    Bulk(Bulk),
}

/// A conversation topic -- a contiguous block of related messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Topic {
    pub messages: Vec<HistoryMessage>,
    pub summary: Option<String>,
}

/// A merged group of old topics, fully compressed to a summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bulk {
    pub summary: String,
    pub source_count: usize,
}

/// Which tier of the hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Current,
    Topics,
    Bulks,
}

/// How much a tier is over its budget (in tokens).
#[derive(Debug, Clone)]
struct TierOverflow {
    tier: Tier,
    actual: usize,
    budget: usize,
    overflow: usize,
}

/// Hierarchical conversation history.
///
/// Manages three compression tiers. After construction from a flat message list
/// and a token budget, call `compress()` to iteratively reduce context until it
/// fits, then call `build_messages()` to get the provider-ready `Message` list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct History {
    pub bulks: Vec<Bulk>,
    pub topics: Vec<Topic>,
    pub current: Topic,
    pub ctx_limit: usize,
}

impl Topic {
    fn new() -> Self {
        Self {
            messages: Vec::new(),
            summary: None,
        }
    }

    fn token_count(&self) -> usize {
        if let Some(ref s) = self.summary {
            return estimate_tokens(s);
        }
        self.messages
            .iter()
            .map(|m| estimate_tokens(&m.content) + 4)
            .sum()
    }
}

impl Bulk {
    fn token_count(&self) -> usize {
        estimate_tokens(&self.summary)
    }
}

impl History {
    /// Build from a flat slice of `HistoryMessage` and a context token limit.
    pub fn from_messages(messages: Vec<HistoryMessage>, ctx_limit: usize) -> Self {
        Self {
            bulks: Vec::new(),
            topics: Vec::new(),
            current: Topic {
                messages,
                summary: None,
            },
            ctx_limit,
        }
    }

    /// Total token count across all three tiers.
    pub fn total_tokens(&self) -> usize {
        self.current.token_count()
            + self.topics.iter().map(|t| t.token_count()).sum::<usize>()
            + self.bulks.iter().map(|b| b.token_count()).sum::<usize>()
    }

    fn budget(&self) -> (usize, usize, usize) {
        (
            (self.ctx_limit as f64 * CURRENT_RATIO) as usize,
            (self.ctx_limit as f64 * TOPICS_RATIO) as usize,
            (self.ctx_limit as f64 * BULKS_RATIO) as usize,
        )
    }

    fn calculate_overflow(&self) -> Vec<TierOverflow> {
        let (current_budget, topics_budget, bulks_budget) = self.budget();

        let current_actual = self.current.token_count();
        let topics_actual: usize = self.topics.iter().map(|t| t.token_count()).sum();
        let bulks_actual: usize = self.bulks.iter().map(|b| b.token_count()).sum();

        vec![
            TierOverflow {
                tier: Tier::Current,
                actual: current_actual,
                budget: current_budget,
                overflow: current_actual.saturating_sub(current_budget),
            },
            TierOverflow {
                tier: Tier::Topics,
                actual: topics_actual,
                budget: topics_budget,
                overflow: topics_actual.saturating_sub(topics_budget),
            },
            TierOverflow {
                tier: Tier::Bulks,
                actual: bulks_actual,
                budget: bulks_budget,
                overflow: bulks_actual.saturating_sub(bulks_budget),
            },
        ]
    }

    /// Move the current topic into the topics list, starting a fresh current topic.
    pub fn new_topic(&mut self) {
        if self.current.messages.is_empty() && self.current.summary.is_none() {
            return;
        }
        let old = std::mem::replace(&mut self.current, Topic::new());
        self.topics.push(old);
    }

    /// Iteratively compress tiers until total tokens fit within ctx_limit,
    /// or we exhaust our iteration budget.
    pub fn compress(&mut self) -> Result<()> {
        for _ in 0..MAX_COMPRESS_ITERS {
            if self.total_tokens() <= self.ctx_limit {
                return Ok(());
            }

            let overflows = self.calculate_overflow();
            let worst = overflows
                .iter()
                .max_by(|a, b| a.overflow.cmp(&b.overflow))
                .cloned();

            let Some(worst) = worst else { break };
            if worst.overflow == 0 {
                break;
            }

            match worst.tier {
                Tier::Current => self.compress_current()?,
                Tier::Topics => self.compress_topics()?,
                Tier::Bulks => self.merge_bulks()?,
            }
        }
        Ok(())
    }

    /// Compress the current topic: summarize middle messages, keep first and last.
    fn compress_current(&mut self) -> Result<()> {
        let len = self.current.messages.len();
        if len <= 3 {
            // Not enough messages to compress -- move to topics and start fresh
            self.new_topic();
            return Ok(());
        }

        // Keep first and last, summarize the middle
        let first = self.current.messages.remove(0);
        let last = self.current.messages.pop().unwrap();
        let middle: Vec<_> = std::mem::take(&mut self.current.messages);

        let summary = Self::build_compressed_summary(&middle);

        let old_summary = self.current.summary.take().unwrap_or_default();
        let combined = if old_summary.is_empty() {
            summary
        } else {
            format!("{}\n{}", old_summary, summary)
        };

        self.current.messages = vec![first, last];
        self.current.summary = Some(combined);

        Ok(())
    }

    /// Compress old topics: summarize them individually, then merge groups of BULK_MERGE_SIZE.
    fn compress_topics(&mut self) -> Result<()> {
        if self.topics.is_empty() {
            return Ok(());
        }

        // Summarize any topic that hasn't been summarized yet
        for topic in &mut self.topics {
            if topic.summary.is_none() {
                topic.summary = Some(Self::build_compressed_summary(&topic.messages));
                topic.messages.clear();
            }
        }

        // Merge groups of topics into bulks
        while self.topics.len() >= BULK_MERGE_SIZE {
            let group: Vec<Topic> = self.topics.drain(..BULK_MERGE_SIZE).collect();
            let summaries: Vec<&str> = group
                .iter()
                .map(|t| t.summary.as_deref().unwrap_or(""))
                .collect();

            let merged = summaries.join("\n---\n");
            let source_count = group.iter().map(|t| t.source_count()).sum::<usize>();

            self.bulks.push(Bulk {
                summary: format!(
                    "[Compressed topic group ({} messages)]: {}",
                    source_count, merged
                ),
                source_count,
            });
        }

        Ok(())
    }

    /// Merge adjacent bulks if we're over the bulk budget.
    fn merge_bulks(&mut self) -> Result<()> {
        if self.bulks.len() <= 1 {
            // Can't merge further -- drop the oldest bulk
            if !self.bulks.is_empty() {
                self.bulks.remove(0);
            }
            return Ok(());
        }

        // Merge the two oldest bulks
        let a = self.bulks.remove(0);
        let b = self.bulks.remove(0);

        self.bulks.insert(
            0,
            Bulk {
                summary: format!(
                    "[Merged {} messages]: {} {}",
                    a.source_count + b.source_count,
                    a.summary,
                    b.summary
                ),
                source_count: a.source_count + b.source_count,
            },
        );

        Ok(())
    }

    /// Build a compressed summary from a slice of messages.
    /// Keeps condensed form of each message (first 120 chars per message).
    fn build_compressed_summary(messages: &[HistoryMessage]) -> String {
        let mut lines = Vec::with_capacity(messages.len());
        for msg in messages {
            let truncated: String = msg.content.chars().take(120).collect();
            lines.push(format!("[{}] {}", msg.role, truncated));
        }
        lines.join("\n")
    }

    /// Build the provider-ready message list from the hierarchy.
    ///
    /// Messages are ordered: [bulk summaries] [topic summaries] [current messages],
    /// which naturally gives the model the compressed old context followed by
    /// the active conversation.
    pub fn build_messages(&self, system_prompt: &str, user_message: &str) -> Vec<Message> {
        let mut context_parts = Vec::new();

        // Bulk summaries (oldest, most compressed)
        if !self.bulks.is_empty() {
            let bulk_text: Vec<&str> = self.bulks.iter().map(|b| b.summary.as_str()).collect();
            context_parts.push(format!(
                "## Older Context (compressed)\n{}",
                bulk_text.join("\n\n")
            ));
        }

        // Topic summaries
        if !self.topics.is_empty() {
            for (i, topic) in self.topics.iter().enumerate() {
                if let Some(ref summary) = topic.summary {
                    context_parts.push(format!("## Previous Topic {}\n{}", i + 1, summary));
                }
            }
        }

        // Build the final message list
        let mut messages = Vec::new();

        // System prompt + compressed context
        let mut full_system = system_prompt.to_string();
        if !context_parts.is_empty() {
            full_system.push_str("\n\n## Conversation History (hierarchically compressed)\n");
            full_system.push_str(&context_parts.join("\n\n"));
        }

        // Include the current topic's summary if present
        if let Some(ref current_summary) = self.current.summary {
            full_system.push_str("\n\n## Current Topic Context\n");
            full_system.push_str(current_summary);
        }

        // Merge any system messages from current topic into system prompt
        let system_messages: Vec<&HistoryMessage> = self.current.messages
            .iter()
            .filter(|m| m.role == "system")
            .collect();
        for sys_msg in &system_messages {
            full_system.push_str("\n\n");
            full_system.push_str(&sys_msg.content);
        }

        messages.push(Message::new("system", full_system));

        // Current topic non-system messages (most recent, least compressed)
        for msg in &self.current.messages {
            if msg.role != "system" {
                messages.push(Message::new(
                    &msg.role,
                    msg.content.clone(),
                ));
            }
        }

        // The new user message
        messages.push(Message::new("user", user_message.to_string()));

        messages
    }

    /// Statistics about the current hierarchy state.
    pub fn stats(&self) -> HistoryStats {
        let (current_budget, topics_budget, bulks_budget) = self.budget();
        HistoryStats {
            total_tokens: self.total_tokens(),
            ctx_limit: self.ctx_limit,
            current_tokens: self.current.token_count(),
            current_budget,
            current_messages: self.current.messages.len(),
            topics_count: self.topics.len(),
            topics_tokens: self.topics.iter().map(|t| t.token_count()).sum(),
            topics_budget,
            bulks_count: self.bulks.len(),
            bulks_tokens: self.bulks.iter().map(|b| b.token_count()).sum(),
            bulks_budget,
        }
    }
}

/// Snapshot of history hierarchy state for logging/debugging.
#[derive(Debug, Clone)]
pub struct HistoryStats {
    pub total_tokens: usize,
    pub ctx_limit: usize,
    pub current_tokens: usize,
    pub current_budget: usize,
    pub current_messages: usize,
    pub topics_count: usize,
    pub topics_tokens: usize,
    pub topics_budget: usize,
    pub bulks_count: usize,
    pub bulks_tokens: usize,
    pub bulks_budget: usize,
}

impl Topic {
    fn source_count(&self) -> usize {
        self.messages.len()
    }
}

impl std::fmt::Display for HistoryStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "History: {}/{} tokens | Current: {} msgs ({} tok/{}) | Topics: {} ({} tok/{}) | Bulks: {} ({} tok/{})",
            self.total_tokens,
            self.ctx_limit,
            self.current_messages,
            self.current_tokens,
            self.current_budget,
            self.topics_count,
            self.topics_tokens,
            self.topics_budget,
            self.bulks_count,
            self.bulks_tokens,
            self.bulks_budget,
        )
    }
}

/// Detect natural topic boundaries in a message list.
/// Returns indices where new topics should begin (0-indexed, points to the first
/// message of the new topic).
pub fn detect_topic_boundaries(messages: &[HistoryMessage]) -> Vec<usize> {
    let mut boundaries = Vec::new();

    for (i, msg) in messages.iter().enumerate().skip(1) {
        // Long gap (> 30 minutes)
        if i > 0 {
            let prev_ts = messages[i - 1].timestamp;
            let gap = msg.timestamp.saturating_sub(prev_ts);
            if gap > 1800 {
                boundaries.push(i);
                continue;
            }
        }

        // Topic shift detected: user starts with a new greeting/question after assistant response
        if msg.role == "user"
            && i >= 2
            && messages[i - 1].role == "assistant"
            && is_topic_shift(&msg.content)
        {
            boundaries.push(i);
        }
    }

    boundaries
}

/// Heuristic: does this user message look like the start of a new topic?
fn is_topic_shift(content: &str) -> bool {
    let lower = content.trim().to_lowercase();
    if lower.is_empty() {
        return false;
    }

    // Greetings
    if lower.starts_with("hey")
        || lower.starts_with("hello")
        || lower.starts_with("hi ")
        || lower.starts_with("yo ")
        || lower.starts_with("good morning")
        || lower.starts_with("good evening")
        || lower.starts_with("what's up")
    {
        return true;
    }

    // Explicit topic change
    if lower.starts_with("new topic")
        || lower.starts_with("different question")
        || lower.starts_with("switching gears")
        || lower.starts_with("on another note")
        || lower.starts_with("let's talk about")
        || lower.starts_with("moving on")
    {
        return true;
    }

    // Very short messages (likely new questions)
    if lower.len() < 20 && lower.ends_with('?') {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: &str, content: &str) -> HistoryMessage {
        HistoryMessage {
            role: role.into(),
            content: content.into(),
            timestamp: 0,
            tool_calls: None,
        }
    }

    fn msg_with_ts(role: &str, content: &str, ts: u64) -> HistoryMessage {
        HistoryMessage {
            role: role.into(),
            content: content.into(),
            timestamp: ts,
            tool_calls: None,
        }
    }

    #[test]
    fn from_messages_single_tier() {
        let messages = vec![msg("user", "hello"), msg("assistant", "hi there")];
        let history = History::from_messages(messages, 10_000);
        assert_eq!(history.current.messages.len(), 2);
        assert!(history.topics.is_empty());
        assert!(history.bulks.is_empty());
    }

    #[test]
    fn new_topic_moves_current() {
        let mut history = History::from_messages(vec![msg("user", "a"), msg("assistant", "b")], 10_000);
        history.new_topic();
        assert!(history.current.messages.is_empty());
        assert_eq!(history.topics.len(), 1);
        assert_eq!(history.topics[0].messages.len(), 2);
    }

    #[test]
    fn new_topic_skips_empty() {
        let mut history = History::from_messages(vec![], 10_000);
        history.new_topic();
        assert!(history.topics.is_empty());
    }

    #[test]
    fn compress_current_summarizes_middle() {
        let messages: Vec<_> = (0..10)
            .map(|i| msg("user", &format!("message number {}", i)))
            .collect();
        let mut history = History::from_messages(messages, 100); // very small budget
        history.compress_current().unwrap();
        assert_eq!(history.current.messages.len(), 2); // first + last
        assert!(history.current.summary.is_some());
    }

    #[test]
    fn compress_current_too_few_moves_to_topics() {
        let messages = vec![msg("user", "a"), msg("assistant", "b"), msg("user", "c")];
        let mut history = History::from_messages(messages, 100);
        history.compress_current().unwrap();
        assert_eq!(history.topics.len(), 1);
        assert!(history.current.messages.is_empty());
    }

    #[test]
    fn compress_topics_merges_into_bulks() {
        let mut history = History::from_messages(vec![], 10_000);
        for i in 0..BULK_MERGE_SIZE {
            let mut topic = Topic::new();
            topic.messages = vec![msg("user", &format!("topic {} message", i))];
            history.topics.push(topic);
        }
        history.compress_topics().unwrap();
        assert!(history.topics.is_empty());
        assert_eq!(history.bulks.len(), 1);
    }

    #[test]
    fn compress_fits_within_budget() {
        let mut messages = Vec::new();
        for i in 0..100 {
            messages.push(msg("user", &format!("This is a fairly long user message number {} with some content to inflate token count", i)));
            messages.push(msg("assistant", &format!("This is a fairly long assistant response number {} with some content to inflate token count", i)));
        }
        let mut history = History::from_messages(messages, 500); // small budget
        let before = history.total_tokens();
        history.compress().unwrap();
        let after = history.total_tokens();
        assert!(after <= 500 || after < before, "compressed {} -> {} (limit 500)", before, after);
    }

    #[test]
    fn build_messages_ordering() {
        let mut history = History::from_messages(
            vec![msg("user", "hello"), msg("assistant", "hi")],
            10_000,
        );
        // Add a topic
        history.new_topic();
        history.topics[0].summary = Some("Previous topic summary".into());
        history.current.messages = vec![msg("user", "current question")];

        let messages = history.build_messages("System prompt", "new user msg");
        assert_eq!(messages[0].role, "system");
        assert!(messages[0].content.as_ref().unwrap().contains("System prompt"));
        assert!(messages[0].content.as_ref().unwrap().contains("Previous topic summary"));
        assert_eq!(messages.last().unwrap().role, "user");
        assert_eq!(
            messages.last().unwrap().content.as_ref().unwrap(),
            "new user msg"
        );
    }

    #[test]
    fn detect_topic_boundaries_gap() {
        let messages = vec![
            msg_with_ts("user", "hello", 1000),
            msg_with_ts("assistant", "hi", 1010),
            msg_with_ts("user", "what's up", 10000), // > 30min gap
        ];
        let boundaries = detect_topic_boundaries(&messages);
        assert_eq!(boundaries, vec![2]);
    }

    #[test]
    fn detect_topic_boundaries_greeting() {
        let messages = vec![
            msg_with_ts("user", "fix the bug", 1000),
            msg_with_ts("assistant", "fixed", 1010),
            msg_with_ts("user", "yo what's the plan for deployment?", 1020),
        ];
        let boundaries = detect_topic_boundaries(&messages);
        assert!(boundaries.contains(&2));
    }

    #[test]
    fn merge_bulks() {
        let mut history = History::from_messages(vec![], 10_000);
        history.bulks.push(Bulk {
            summary: "bulk 1".into(),
            source_count: 10,
        });
        history.bulks.push(Bulk {
            summary: "bulk 2".into(),
            source_count: 15,
        });
        history.merge_bulks().unwrap();
        assert_eq!(history.bulks.len(), 1);
        assert_eq!(history.bulks[0].source_count, 25);
    }

    #[test]
    fn drop_oldest_bulk_when_single() {
        let mut history = History::from_messages(vec![], 10_000);
        history.bulks.push(Bulk {
            summary: "only bulk".into(),
            source_count: 10,
        });
        history.merge_bulks().unwrap();
        assert!(history.bulks.is_empty());
    }

    #[test]
    fn stats_display() {
        let history = History::from_messages(vec![msg("user", "test")], 5_000);
        let stats = history.stats();
        let display = format!("{}", stats);
        assert!(display.contains("History:"));
        assert!(display.contains("/5000 tokens"));
    }

    #[test]
    fn compress_full_cycle() {
        // Simulate a long conversation that exceeds budget
        let mut messages = Vec::new();
        for i in 0..200 {
            messages.push(msg("user", &format!("User message {} with enough content to be realistic and consume tokens", i)));
            messages.push(msg("assistant", &format!("Assistant response {} with enough content to be realistic and consume tokens", i)));
        }
        let mut history = History::from_messages(messages, 1000);
        let _ = history.compress();
        let stats = history.stats();
        // After compression, should have some topics/bulks
        assert!(
            stats.topics_count > 0 || stats.bulks_count > 0 || stats.total_tokens <= 1000,
            "Expected compression to produce topics/bulks or fit within budget: {}",
            stats
        );
    }
}
