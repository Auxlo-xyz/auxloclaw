//! Session history tools — lets the agent list and search past conversations.
//!
//! These tools give the agent long-term memory across sessions. The agent should
//! use them proactively whenever the user references something not present in the
//! current conversation (a past project, a previous decision, an old config, etc.).
//!
//! Usage rules for the agent:
//! - Search silently. Never tell the user "I searched past sessions" or quote raw
//!   search results back to them.
//! - Use the retrieved context to inform your response naturally, as if you simply
//!   remembered it. The user should never see the tool call or its output directly.
//! - If the user says "as we discussed before", "that thing from last time",
//!   "remember when we...", or mentions anything you don't have in the current
//!   conversation context, call `search_sessions` first before responding.
//! - Prefer `search_sessions` for keyword/topic lookup and `list_sessions` for
//!   browsing recent activity or finding session IDs to recover.

use crate::orchestrator::{Tool, ToolResult};
use async_trait::async_trait;
use anyhow::{Result, anyhow};
use std::sync::Arc;
use std::time::Instant;

/// List past sessions from the SQLite store
pub struct ListSessionsTool {
    store: Arc<crate::memory::MemoryStore>,
}

impl ListSessionsTool {
    pub fn new(store: Arc<crate::memory::MemoryStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for ListSessionsTool {
    fn name(&self) -> &str { "list_sessions" }
    fn description(&self) -> &str {
        "List past conversation sessions with metadata. Shows session ID, channel, \
         message count, user goal, completion status, and last activity. \
         Use this to browse recent sessions or find a session ID to recover."
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": "integer",
                    "description": "Max sessions to return (default 20)"
                },
                "channel": {
                    "type": "string",
                    "description": "Filter by channel name (telegram, discord, cli, code)"
                }
            }
        })
    }
    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let start = Instant::now();
        let limit = args["limit"].as_u64().unwrap_or(20) as usize;
        let channel_filter = args["channel"].as_str();

        let sessions = self.store.list_sessions(limit)
            .map_err(|e| anyhow!("Failed to list sessions: {}", e))?;

        let filtered: Vec<_> = if let Some(ch) = channel_filter {
            sessions.into_iter().filter(|s| s.channel == ch).collect()
        } else {
            sessions
        };

        if filtered.is_empty() {
            return Ok(ToolResult {
                tool_name: "list_sessions".to_string(),
                output: serde_json::json!("No sessions found."),
                success: true,
                error: None,
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }

        let mut out = format!("Found {} sessions:\n\n", filtered.len());
        for s in &filtered {
            let goal = s.user_goal.as_deref().unwrap_or("-");
            let status = s.completed.as_deref().unwrap_or("unknown");
            out.push_str(&format!(
                "- **{}** ({}) — {} msgs, goal: {}, status: {}, updated: {}\n",
                s.session_id, s.channel, s.message_count, goal, status, s.updated_at
            ));
            if let Some(ref next) = s.next_steps {
                if !next.is_empty() {
                    out.push_str(&format!("  next: {}\n", next));
                }
            }
        }

        Ok(ToolResult {
            tool_name: "list_sessions".to_string(),
            output: serde_json::json!(out),
            success: true,
            error: None,
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }
}

/// Search session message history by keyword
pub struct SearchSessionsTool {
    store: Arc<crate::memory::MemoryStore>,
}

impl SearchSessionsTool {
    pub fn new(store: Arc<crate::memory::MemoryStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for SearchSessionsTool {
    fn name(&self) -> &str { "search_sessions" }
    fn description(&self) -> &str {
        "Search through all past conversation messages by keyword. Returns matching \
         messages with their session context. Use this when the user references \
         something from a previous conversation that isn't in your current context \
         (e.g. 'as we discussed', 'that config from last time', 'remember when'). \
         Retrieved context should be woven into your response naturally — never quote \
         raw results to the user or mention that you searched."
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Keyword or phrase to search for in session messages"
                },
                "session_id": {
                    "type": "string",
                    "description": "Optional: limit search to a specific session"
                },
                "limit": {
                    "type": "integer",
                    "description": "Max results to return (default 20)"
                }
            },
            "required": ["query"]
        })
    }
    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let start = Instant::now();
        let query = args["query"].as_str()
            .ok_or_else(|| anyhow!("Missing required parameter: query"))?;
        let session_id = args["session_id"].as_str();
        let limit = args["limit"].as_u64().unwrap_or(20) as usize;

        let results = self.store.search_messages(query, session_id, limit)
            .map_err(|e| anyhow!("Search failed: {}", e))?;

        if results.is_empty() {
            return Ok(ToolResult {
                tool_name: "search_sessions".to_string(),
                output: serde_json::json!(format!("No messages matching '{}' found.", query)),
                success: true,
                error: None,
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }

        let mut out = format!("Found {} messages matching '{}':\n\n", results.len(), query);
        for msg in &results {
            let preview: String = msg.content.chars().take(300).collect();
            out.push_str(&format!(
                "- [{}] {}: {}\n",
                msg.session_id, msg.role, preview
            ));
        }

        Ok(ToolResult {
            tool_name: "search_sessions".to_string(),
            output: serde_json::json!(out),
            success: true,
            error: None,
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }
}
