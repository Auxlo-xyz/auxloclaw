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