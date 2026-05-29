//! Shared Blackboard - Inter-agent shared state for multi-agent coordination
//!
//! A thread-safe key-value store with TTL support that allows sub-agents
//! to communicate, share findings, and coordinate on complex tasks.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

/// A single entry on the blackboard
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlackboardEntry {
    pub key: String,
    pub value: serde_json::Value,
    pub author: String,
    pub created_at: u64,
    pub expires_at: Option<u64>,
    pub tags: Vec<String>,
}

/// Shared blackboard for multi-agent coordination
///
/// Agents can post findings, read shared state, and coordinate
/// through this shared data structure. Entries can have TTLs
/// and tags for organization.
#[derive(Clone)]
pub struct SharedBlackboard {
    entries: Arc<RwLock<HashMap<String, BlackboardEntry>>>,
    channels: Arc<RwLock<HashMap<String, Vec<BlackboardMessage>>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlackboardMessage {
    pub from: String,
    pub to: Option<String>,
    pub content: serde_json::Value,
    pub timestamp: u64,
    pub topic: String,
}

impl SharedBlackboard {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            channels: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Write a value to the blackboard
    pub async fn write(&self, key: &str, value: serde_json::Value, author: &str, ttl_secs: Option<u64>, tags: Vec<String>) {
        let now = now_epoch();
        let entry = BlackboardEntry {
            key: key.to_string(),
            value,
            author: author.to_string(),
            created_at: now,
            expires_at: ttl_secs.map(|t| now + t),
            tags,
        };
        self.entries.write().await.insert(key.to_string(), entry);
    }

    /// Read a value from the blackboard
    pub async fn read(&self, key: &str) -> Option<serde_json::Value> {
        let entries = self.entries.read().await;
        entries.get(key).and_then(|e| {
            if is_expired(e) {
                None
            } else {
                Some(e.value.clone())
            }
        })
    }

    /// Read all entries matching a tag
    pub async fn read_by_tag(&self, tag: &str) -> Vec<BlackboardEntry> {
        let entries = self.entries.read().await;
        let now = now_epoch();
        entries.values()
            .filter(|e| e.tags.contains(&tag.to_string()) && !is_expired_at(e, now))
            .cloned()
            .collect()
    }

    /// List all non-expired keys
    pub async fn list_keys(&self) -> Vec<String> {
        let entries = self.entries.read().await;
        let now = now_epoch();
        entries.values()
            .filter(|e| !is_expired_at(e, now))
            .map(|e| e.key.clone())
            .collect()
    }

    /// Delete a key
    pub async fn delete(&self, key: &str) -> bool {
        self.entries.write().await.remove(key).is_some()
    }

    /// Post a message to a named channel (for agent-to-agent messaging)
    pub async fn post_message(&self, channel: &str, from: &str, to: Option<&str>, topic: &str, content: serde_json::Value) {
        let msg = BlackboardMessage {
            from: from.to_string(),
            to: to.map(|s| s.to_string()),
            content,
            timestamp: now_epoch(),
            topic: topic.to_string(),
        };
        self.channels.write().await
            .entry(channel.to_string())
            .or_default()
            .push(msg);
    }

    /// Read messages from a channel, optionally filtered by topic
    pub async fn read_messages(&self, channel: &str, topic: Option<&str>, since: Option<u64>) -> Vec<BlackboardMessage> {
        let channels = self.channels.read().await;
        channels.get(channel)
            .map(|msgs| {
                msgs.iter()
                    .filter(|m| topic.map_or(true, |t| m.topic == t))
                    .filter(|m| since.map_or(true, |ts| m.timestamp >= ts))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Clean up expired entries
    pub async fn cleanup(&self) -> usize {
        let now = now_epoch();
        let mut entries = self.entries.write().await;
        let before = entries.len();
        entries.retain(|_, e| !is_expired_at(e, now));
        before - entries.len()
    }

    /// Snapshot the entire blackboard for inspection
    pub async fn snapshot(&self) -> Vec<BlackboardEntry> {
        let entries = self.entries.read().await;
        let now = now_epoch();
        entries.values()
            .filter(|e| !is_expired_at(e, now))
            .cloned()
            .collect()
    }
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn is_expired(entry: &BlackboardEntry) -> bool {
    is_expired_at(entry, now_epoch())
}

fn is_expired_at(entry: &BlackboardEntry, now: u64) -> bool {
    entry.expires_at.map_or(false, |exp| now >= exp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn write_and_read_entry() {
        let bb = SharedBlackboard::new();
        bb.write("key1", json!("value1"), "agent_a", None, vec![]).await;
        let val = bb.read("key1").await;
        assert_eq!(val, Some(json!("value1")));
    }

    #[tokio::test]
    async fn expired_entry_returns_none() {
        let bb = SharedBlackboard::new();
        bb.write("key1", json!("value1"), "agent_a", Some(0), vec![]).await;
        // TTL of 0 means already expired
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        assert!(bb.read("key1").await.is_none());
    }

    #[tokio::test]
    async fn tag_filtering() {
        let bb = SharedBlackboard::new();
        bb.write("a", json!(1), "x", None, vec!["research".into()]).await;
        bb.write("b", json!(2), "y", None, vec!["code".into()]).await;
        let results = bb.read_by_tag("research").await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key, "a");
    }

    #[tokio::test]
    async fn message_passing() {
        let bb = SharedBlackboard::new();
        bb.post_message("task1", "agent_a", Some("agent_b"), "findings", json!({"result": 42})).await;
        let msgs = bb.read_messages("task1", Some("findings"), None).await;
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].from, "agent_a");
    }

    #[tokio::test]
    async fn cleanup_removes_expired() {
        let bb = SharedBlackboard::new();
        bb.write("old", json!("data"), "x", Some(0), vec![]).await;
        bb.write("new", json!("data"), "x", Some(3600), vec![]).await;
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let removed = bb.cleanup().await;
        assert_eq!(removed, 1);
        assert_eq!(bb.list_keys().await.len(), 1);
    }
}
