//! Blackboard tool - lets agents read/write shared state and communicate

use crate::coordination::blackboard::SharedBlackboard;
use crate::orchestrator::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;

/// Tool for agents to interact with the shared blackboard
pub struct BlackboardTool {
    blackboard: SharedBlackboard,
}

impl BlackboardTool {
    pub fn new(blackboard: SharedBlackboard) -> Self {
        Self { blackboard }
    }
}

#[async_trait]
impl Tool for BlackboardTool {
    fn name(&self) -> &str {
        "blackboard"
    }

    fn description(&self) -> &str {
        "Read/write shared state on the multi-agent blackboard. Use this to share findings \
         between sub-agents, post intermediate results, read what other agents discovered, \
         or send messages to other agents on a task. Actions: write, read, list, delete, \
         post_message, read_messages, snapshot."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["write", "read", "list", "delete", "post_message", "read_messages", "snapshot"],
                    "description": "Action to perform"
                },
                "key": {
                    "type": "string",
                    "description": "Key for write/read/delete operations"
                },
                "value": {
                    "description": "Value to write (any JSON)"
                },
                "author": {
                    "type": "string",
                    "description": "Agent ID performing the write (e.g. 'researcher_abc')"
                },
                "ttl_secs": {
                    "type": "integer",
                    "description": "Time-to-live in seconds (optional, 0 = no expiry)"
                },
                "tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Tags for organizing entries"
                },
                "tag": {
                    "type": "string",
                    "description": "Tag to filter by (for read action)"
                },
                "channel": {
                    "type": "string",
                    "description": "Channel name for messaging"
                },
                "from": {
                    "type": "string",
                    "description": "Sender agent ID (for post_message)"
                },
                "to": {
                    "type": "string",
                    "description": "Recipient agent ID (optional, for post_message)"
                },
                "topic": {
                    "type": "string",
                    "description": "Message topic (for post_message/read_messages)"
                },
                "since": {
                    "type": "integer",
                    "description": "Unix timestamp - only return messages after this time"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let action = args["action"].as_str().unwrap_or("");

        let result = match action {
            "write" => {
                let key = args["key"].as_str().unwrap_or("");
                let value = args.get("value").cloned().unwrap_or(json!(null));
                let author = args["author"].as_str().unwrap_or("unknown");
                let ttl = args["ttl_secs"].as_u64();
                let tags: Vec<String> = args["tags"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();

                if key.is_empty() {
                    return Ok(tool_error(self.name(), "key is required for write"));
                }

                self.blackboard.write(key, value, author, ttl, tags).await;
                json!({"status": "written", "key": key})
            }
            "read" => {
                let key = args["key"].as_str().unwrap_or("");
                let tag = args["tag"].as_str();

                if let Some(tag) = tag {
                    let entries = self.blackboard.read_by_tag(tag).await;
                    json!({
                        "entries": entries.iter().map(|e| json!({
                            "key": e.key,
                            "value": e.value,
                            "author": e.author,
                            "tags": e.tags,
                        })).collect::<Vec<_>>()
                    })
                } else if !key.is_empty() {
                    match self.blackboard.read(key).await {
                        Some(val) => json!({"key": key, "value": val}),
                        None => json!({"key": key, "value": null, "note": "not found or expired"}),
                    }
                } else {
                    return Ok(tool_error(self.name(), "key or tag is required for read"));
                }
            }
            "list" => {
                let keys = self.blackboard.list_keys().await;
                json!({"keys": keys, "count": keys.len()})
            }
            "delete" => {
                let key = args["key"].as_str().unwrap_or("");
                if key.is_empty() {
                    return Ok(tool_error(self.name(), "key is required for delete"));
                }
                let deleted = self.blackboard.delete(key).await;
                json!({"deleted": deleted, "key": key})
            }
            "post_message" => {
                let channel = args["channel"].as_str().unwrap_or("default");
                let from = args["from"].as_str().unwrap_or("unknown");
                let to = args["to"].as_str();
                let topic = args["topic"].as_str().unwrap_or("general");
                let content = args.get("value").cloned().unwrap_or(json!(null));

                self.blackboard.post_message(channel, from, to, topic, content).await;
                json!({"status": "posted", "channel": channel, "from": from, "topic": topic})
            }
            "read_messages" => {
                let channel = args["channel"].as_str().unwrap_or("default");
                let topic = args["topic"].as_str();
                let since = args["since"].as_u64();

                let msgs = self.blackboard.read_messages(channel, topic, since).await;
                json!({
                    "messages": msgs.iter().map(|m| json!({
                        "from": m.from,
                        "to": m.to,
                        "topic": m.topic,
                        "content": m.content,
                        "timestamp": m.timestamp,
                    })).collect::<Vec<_>>(),
                    "count": msgs.len(),
                })
            }
            "snapshot" => {
                let entries = self.blackboard.snapshot().await;
                json!({
                    "entries": entries.iter().map(|e| json!({
                        "key": e.key,
                        "value": e.value,
                        "author": e.author,
                        "tags": e.tags,
                    })).collect::<Vec<_>>(),
                    "count": entries.len(),
                })
            }
            _ => {
                return Ok(tool_error(self.name(), &format!("Unknown action: {}", action)));
            }
        };

        Ok(ToolResult {
            tool_name: self.name().into(),
            success: true,
            output: result,
            error: None,
            duration_ms: 0,
        })
    }
}

// --- OrchestrateTool ---

use crate::coordination::AgentCoordinator;
use std::sync::Arc;
use tokio::sync::RwLock as TokioRwLock;

/// Tool for orchestrating multiple sub-agents on a collaborative task
pub struct OrchestrateTool {
    coordinator: Arc<TokioRwLock<Option<Arc<AgentCoordinator>>>>,
    blackboard: SharedBlackboard,
}

impl OrchestrateTool {
    pub fn new(
        coordinator: Arc<TokioRwLock<Option<Arc<AgentCoordinator>>>>,
        blackboard: SharedBlackboard,
    ) -> Self {
        Self { coordinator, blackboard }
    }
}

#[async_trait]
impl Tool for OrchestrateTool {
    fn name(&self) -> &str {
        "orchestrate"
    }

    fn description(&self) -> &str {
        "Launch multiple sub-agents that collaborate on a complex task through the shared blackboard. \
         Each sub-agent gets its own task and can read/write shared state. Use this for complex \
         multi-step research, analysis, or tasks that benefit from parallel specialist work with \
         shared findings. The orchestrator monitors all agents and returns aggregated results."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "task_name": {
                    "type": "string",
                    "description": "Name for this orchestration task (used as blackboard channel)"
                },
                "agents": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "agent_type": {
                                "type": "string",
                                "enum": ["researcher", "coder", "analyst", "planner", "reviewer", "general"],
                                "description": "Specialist type"
                            },
                            "task": {
                                "type": "string",
                                "description": "Specific task for this agent"
                            }
                        },
                        "required": ["agent_type", "task"]
                    },
                    "description": "List of sub-agents to spawn"
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "Overall timeout in seconds (default: 600)",
                    "minimum": 120,
                    "maximum": 1800
                }
            },
            "required": ["task_name", "agents"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let task_name = args["task_name"].as_str().unwrap_or("task");
        let agents = args["agents"].as_array();
        let timeout_secs = args["timeout_secs"].as_u64().unwrap_or(600);

        let agents = match agents {
            Some(a) if !a.is_empty() => a,
            _ => {
                return Ok(ToolResult {
                    tool_name: self.name().into(),
                    success: false,
                    output: json!({"error": "agents array is required and must not be empty"}),
                    error: Some("Missing agents parameter".into()),
                    duration_ms: 0,
                });
            }
        };

        let coordinator_guard = self.coordinator.read().await;
        let coordinator = match coordinator_guard.as_ref() {
            Some(c) => c.clone(),
            None => {
                return Ok(ToolResult {
                    tool_name: self.name().into(),
                    success: false,
                    output: json!({"error": "Sub-agent system not initialized"}),
                    error: Some("Coordinator not available".into()),
                    duration_ms: 0,
                });
            }
        };
        drop(coordinator_guard);

        // Post the orchestration task description to the blackboard
        self.blackboard.write(
            &format!("{}:goal", task_name),
            json!({"task_name": task_name, "agent_count": agents.len()}),
            "orchestrator",
            Some(timeout_secs + 60),
            vec![task_name.to_string(), "meta".to_string()],
        ).await;

        // Build task list for parallel execution
        let mut tasks = Vec::new();
        for agent_def in agents {
            let agent_type = agent_def["agent_type"].as_str().unwrap_or("general").to_string();
            let task = agent_def["task"].as_str().unwrap_or("").to_string();

            // Inject blackboard context into the task
            let enhanced_task = format!(
                "{}\n\n--- SHARED BLACKBOARD CONTEXT ---\n\
                 You have access to the 'blackboard' tool. Use it to:\n\
                 - Write your findings: blackboard action=write key=\"{}:your_findings\" value=<your data> author=\"{}\"\n\
                 - Read other agents' work: blackboard action=read key=\"<agent_key>\"\n\
                 - Post messages: blackboard action=post_message channel=\"{}\" from=\"{}\" topic=\"<topic>\"\n\
                 - List all shared data: blackboard action=list\n\
                 \nTask channel: {}\n\
                 Share your intermediate results so other agents can build on them.",
                task, task_name, agent_type, task_name, agent_type, task_name
            );

            tasks.push((enhanced_task, agent_type));
        }

        let start = std::time::Instant::now();
        let results = coordinator.execute_parallel_sub_agents(tasks).await;

        let elapsed = start.elapsed().as_millis() as u64;

        // Collect final blackboard state for this task
        let final_state = self.blackboard.read_by_tag(task_name).await;
        let board_snapshot: Vec<serde_json::Value> = final_state.iter().map(|e| {
            json!({"key": e.key, "value": e.value, "author": e.author})
        }).collect();

        Ok(ToolResult {
            tool_name: self.name().into(),
            success: true,
            output: json!({
                "task_name": task_name,
                "agent_results": results,
                "blackboard_state": board_snapshot,
                "total_agents": results.len(),
                "duration_ms": elapsed,
            }),
            error: None,
            duration_ms: elapsed,
        })
    }
}

fn tool_error(tool_name: &str, msg: &str) -> ToolResult {
    ToolResult {
        tool_name: tool_name.into(),
        success: false,
        output: json!({"error": msg}),
        error: Some(msg.into()),
        duration_ms: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn blackboard_tool_write_and_read() {
        let bb = SharedBlackboard::new();
        let tool = BlackboardTool::new(bb.clone());

        // Write
        let result = tool.execute(json!({
            "action": "write",
            "key": "test_key",
            "value": {"result": 42},
            "author": "test_agent"
        })).await.unwrap();
        assert!(result.success);

        // Read
        let result = tool.execute(json!({
            "action": "read",
            "key": "test_key"
        })).await.unwrap();
        assert!(result.success);
        assert_eq!(result.output["value"]["result"], 42);
    }

    #[tokio::test]
    async fn blackboard_tool_list() {
        let bb = SharedBlackboard::new();
        let tool = BlackboardTool::new(bb.clone());

        tool.execute(json!({"action": "write", "key": "a", "value": 1, "author": "x"})).await.unwrap();
        tool.execute(json!({"action": "write", "key": "b", "value": 2, "author": "y"})).await.unwrap();

        let result = tool.execute(json!({"action": "list"})).await.unwrap();
        assert_eq!(result.output["count"], 2);
    }
}
