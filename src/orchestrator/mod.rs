//! Tool Orchestrator
use anyhow::Result;
use async_trait::async_trait;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;
    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult>;
}

pub struct ToolResult {
    pub tool_name: String,
    pub success: bool,
    pub output: serde_json::Value,
    pub error: Option<String>,
    pub duration_ms: u64,
}

pub struct ToolDefinition {
    pub tool_type: String,
    pub function: FunctionDef,
}

pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

pub struct ToolOrchestrator {
    registry: DashMap<String, Arc<dyn Tool>>,
}

impl ToolOrchestrator {
    pub fn new() -> Self {
        let orch = Self { registry: DashMap::new() };
        orch.register_builtin();
        orch
    }
    
    fn register_builtin(&self) {
        self.register(Arc::new(ScriptTool));
        self.register(Arc::new(ParallelTool));
    }

    pub fn register(&self, tool: Arc<dyn Tool>) {
        self.registry.insert(tool.name().to_string(), tool);
    }

    pub fn get_definitions(&self) -> Vec<ToolDefinition> {
        self.registry.iter().map(|t| ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: t.name().to_string(),
                description: t.description().to_string(),
                parameters: t.parameters(),
            },
        }).collect()
    }

    pub async fn execute_tool(&self, name: &str, args: serde_json::Value) -> ToolResult {
        let start = std::time::Instant::now();
        let result = if let Some(tool) = self.registry.get(name) {
            tool.execute(args).await
        } else {
            Err(anyhow::anyhow!("Tool not found: {}", name))
        };
        ToolResult {
            tool_name: name.into(),
            success: result.is_ok(),
            output: result.as_ref().ok().map(|r| r.output.clone()).unwrap_or(serde_json::Value::Null),
            error: result.as_ref().err().map(|e| e.to_string()),
            duration_ms: start.elapsed().as_millis() as u64,
        }
    }
}

pub struct ScriptTool;
#[async_trait]
impl Tool for ScriptTool {
    fn name(&self) -> &str { "execute_script" }
    fn description(&self) -> &str { "Execute Python/TypeScript/Bash scripts" }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "language": {"type": "string", "enum": ["python", "typescript", "shell"]},
                "code": {"type": "string"}
            },
            "required": ["language", "code"]
        })
    }
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
            Ok(o) => Ok(ToolResult {
                tool_name: self.name().into(),
                success: o.status.success(),
                output: serde_json::json!({
                    "stdout": String::from_utf8_lossy(&o.stdout),
                    "stderr": String::from_utf8_lossy(&o.stderr)
                }),
                error: None,
                duration_ms: 0,
            }),
            Err(e) => Ok(ToolResult {
                tool_name: self.name().into(),
                success: false,
                output: serde_json::Value::Null,
                error: Some(e.to_string()),
                duration_ms: 0,
            }),
        }
    }
}

pub struct ParallelTool;
#[async_trait]
impl Tool for ParallelTool {
    fn name(&self) -> &str { "execute_parallel" }
    fn description(&self) -> &str { "Execute multiple commands in parallel" }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "commands": {
                    "type": "array",
                    "items": {"type": "string"}
                }
            },
            "required": ["commands"]
        })
    }
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
                Ok(Ok(o)) => results.push(serde_json::json!({
                    "success": o.status.success(),
                    "stdout": String::from_utf8_lossy(&o.stdout),
                    "stderr": String::from_utf8_lossy(&o.stderr)
                })),
                _ => results.push(serde_json::json!({
                    "success": false,
                    "error": "execution failed"
                })),
            }
        }
        Ok(ToolResult {
            tool_name: self.name().into(),
            success: true,
            output: serde_json::json!({ "results": results }),
            error: None,
            duration_ms: 0,
        })
    }
}
