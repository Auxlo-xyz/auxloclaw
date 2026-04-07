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
    fn description(&self) -> &str { "Fetch URL content" }
    fn parameters(&self) -> serde_json::Value { serde_json::json!({"type": "object", "properties": {"url": {"type": "string"}}}) }
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

// Script execution tool
pub struct ScriptTool;

#[async_trait]
impl Tool for ScriptTool {
    fn name(&self) -> &str { "execute_script" }
    fn description(&self) -> &str { "Execute Python/TypeScript/Bash scripts" }
    fn parameters(&self) -> serde_json::Value { serde_json::json!({"type": "object"}) }
    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let lang = args["language"].as_str().unwrap_or("shell");
        let code = args["code"].as_str().unwrap_or("");
        let ext = match lang { "python" => "py", "typescript" => "ts", _ => "sh" };
        let tmp = std::env::temp_dir().join(format!("script.{}", ext));
        tokio::fs::write(&tmp, code).await?;
        let runner = match lang {
            "python" => format!("python3 {}", tmp.display()),
            "typescript" => format!("bun {}", tmp.display()),
            _ => format!("sh {}", tmp.display()),
        };
        let output = tokio::process::Command::new("sh").arg("-c").arg(&runner).output().await;
        let _ = tokio::fs::remove_file(&tmp).await;
        match output {
            Ok(o) => Ok(ToolResult { tool_name: self.name().into(), success: o.status.success(), output: serde_json::json!({"stdout": String::from_utf8_lossy(&o.stdout), "stderr": String::from_utf8_lossy(&o.stderr)}), error: None, duration_ms: 0 }),
            Err(e) => Ok(ToolResult { tool_name: self.name().into(), success: false, output: serde_json::Value::Null, error: Some(e.to_string()), duration_ms: 0 }),
        }
    }
}

// Parallel execution tool
pub struct ParallelTool;

#[async_trait]
impl Tool for ParallelTool {
    fn name(&self) -> &str { "execute_parallel" }
    fn description(&self) -> &str { "Execute commands in parallel" }
    fn parameters(&self) -> serde_json::Value { serde_json::json!({"type": "object"}) }
    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let cmds: Vec<String> = serde_json::from_value(args["commands"].clone()).unwrap_or_default();
        let handles: Vec<_> = cmds.iter().map(|c| {
            let cmd = c.clone();
            tokio::spawn(async move {
                tokio::process::Command::new("sh").arg("-c").arg(&cmd).output().await
            })
        }).collect();
        let mut results = Vec::new();
        for h in handles {
            match h.await {
                Ok(Ok(o)) => results.push(serde_json::json!({"success": o.status.success(), "stdout": String::from_utf8_lossy(&o.stdout), "stderr": String::from_utf8_lossy(&o.stderr)})),
                _ => results.push(serde_json::json!({"success": false, "error": "execution failed"})),
            }
        }
        Ok(ToolResult { tool_name: self.name().into(), success: true, output: serde_json::json!({"results": results}), error: None, duration_ms: 0 })
    }
}