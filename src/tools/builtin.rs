//! Built-in tools

// Tools are defined in orchestrator/mod.rs for simplicity
// This module would contain additional specialized tools

use crate::orchestrator::{Tool, ToolResult};
use async_trait::async_trait;
use anyhow::{anyhow, Result};

/// HTTP fetch tool
pub struct HttpFetchTool {
    client: reqwest::Client,
}

impl HttpFetchTool {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl Tool for HttpFetchTool {
    fn name(&self) -> &str { "http_fetch" }
    fn description(&self) -> &str { "Fetch content from a URL" }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {"type": "string", "description": "URL to fetch"},
                "method": {"type": "string", "enum": ["GET", "POST"], "default": "GET"},
                "headers": {"type": "object", "description": "Optional headers"}
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let url = args["url"].as_str()
            .ok_or_else(|| anyhow!("Missing url parameter"))?;
        
        let response = self.client.get(url).send().await
            .map_err(|e| anyhow!("Request failed: {}", e))?;
        
        let status = response.status().as_u16();
        let body = response.text().await
            .map_err(|e| anyhow!("Failed to read response: {}", e))?;

        Ok(ToolResult {
            tool_name: self.name().to_string(),
            success: status >= 200 && status < 300,
            output: serde_json::json!({
                "status": status,
                "body": body.chars().take(10000).collect::<String>()
            }),
            error: None,
            duration_ms: 0,
        })
    }
}

/// Memory tool
pub struct MemoryTool {
    memory: std::sync::Arc<crate::memory::MemoryEngine>,
}

impl MemoryTool {
    pub fn new(memory: std::sync::Arc<crate::memory::MemoryEngine>) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for MemoryTool {
    fn name(&self) -> &str { "memory" }
    fn description(&self) -> &str { "Store or retrieve memories" }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["store", "retrieve", "search"]},
                "key": {"type": "string", "description": "Memory key"},
                "value": {"type": "string", "description": "Value to store (for store action)"},
                "query": {"type": "string", "description": "Search query (for search action)"}
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let action = args["action"].as_str()
            .ok_or_else(|| anyhow!("Missing action parameter"))?;

        match action {
            "store" => {
                let key = args["key"].as_str()
                    .ok_or_else(|| anyhow!("Missing key parameter"))?;
                let value = args["value"].as_str()
                    .ok_or_else(|| anyhow!("Missing value parameter"))?;
                
                self.memory.store(key, value, None).await?;
                
                Ok(ToolResult {
                    tool_name: self.name().to_string(),
                    success: true,
                    output: serde_json::json!({"stored": true}),
                    error: None,
                    duration_ms: 0,
                })
            }
            "retrieve" => {
                let key = args["key"].as_str()
                    .ok_or_else(|| anyhow!("Missing key parameter"))?;
                
                let value = self.memory.retrieve(key).await;
                
                Ok(ToolResult {
                    tool_name: self.name().to_string(),
                    success: value.is_some(),
                    output: serde_json::json!({
                        "found": value.is_some(),
                        "value": value.map(|e| e.content)
                    }),
                    error: None,
                    duration_ms: 0,
                })
            }
            "search" => {
                let query = args["query"].as_str()
                    .ok_or_else(|| anyhow!("Missing query parameter"))?;
                
                let results = self.memory.search(query, 5).await?;
                
                Ok(ToolResult {
                    tool_name: self.name().to_string(),
                    success: true,
                    output: serde_json::json!({
                        "results": results.iter().map(|e| serde_json::json!({
                            "key": e.key,
                            "content": e.content
                        })).collect::<Vec<_>>()
                    }),
                    error: None,
                    duration_ms: 0,
                })
            }
            _ => Err(anyhow!("Unknown action: {}", action))
        }
    }
}