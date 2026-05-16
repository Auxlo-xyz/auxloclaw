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
    approval_policy: crate::tools::approval::ApprovalPolicy,
}

impl ToolOrchestrator {
    pub fn new() -> Self {
        let orch = Self {
            registry: DashMap::new(),
            approval_policy: crate::tools::approval::ApprovalPolicy::from_env(),
        };
        orch.register_builtin();
        orch
    }

    fn register_builtin(&self) {
        use crate::tools::ExecuteCodeTool;
        self.register(Arc::new(ExecuteCodeTool::new()));

        // Register web tools
        use crate::tools::web::{
            BrowserClickTool, BrowserCloseTool, BrowserFillTool, BrowserGetTool, BrowserOpenTool,
            BrowserScreenshotTool, BrowserSnapshotTool, WebSearchTool, XFetchTool,
        };
        self.register(Arc::new(WebSearchTool));
        self.register(Arc::new(BrowserOpenTool));
        self.register(Arc::new(BrowserSnapshotTool));
        self.register(Arc::new(BrowserClickTool));
        self.register(Arc::new(BrowserFillTool));
        self.register(Arc::new(BrowserScreenshotTool));
        self.register(Arc::new(BrowserGetTool));
        self.register(Arc::new(BrowserCloseTool));
        self.register(Arc::new(XFetchTool));
    }

    pub fn register(&self, tool: Arc<dyn Tool>) {
        self.registry.insert(tool.name().to_string(), tool);
    }

    pub fn get_definitions(&self) -> Vec<ToolDefinition> {
        self.registry
            .iter()
            .map(|t| ToolDefinition {
                tool_type: "function".into(),
                function: FunctionDef {
                    name: t.name().to_string(),
                    description: t.description().to_string(),
                    parameters: t.parameters(),
                },
            })
            .collect()
    }

    pub async fn execute_tool(&self, name: &str, args: serde_json::Value) -> ToolResult {
        let start = std::time::Instant::now();

        let decision = self.approval_policy.evaluate_tool(name, &args);
        if !decision.allowed {
            return ToolResult {
                tool_name: name.into(),
                success: false,
                output: serde_json::json!({
                    "requires_approval": decision.requires_approval,
                    "risk": format!("{:?}", decision.risk).to_lowercase(),
                    "reason": decision.reason,
                }),
                error: Some("tool execution blocked by approval policy".to_string()),
                duration_ms: start.elapsed().as_millis() as u64,
            };
        }

        let result = if let Some(tool) = self.registry.get(name) {
            tool.execute(args).await
        } else {
            Err(anyhow::anyhow!("Tool not found: {}", name))
        };
        ToolResult {
            tool_name: name.into(),
            success: result.is_ok(),
            output: result
                .as_ref()
                .ok()
                .map(|r| r.output.clone())
                .unwrap_or(serde_json::Value::Null),
            error: result.as_ref().err().map(|e| e.to_string()),
            duration_ms: start.elapsed().as_millis() as u64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn approval_blocks_destructive_tool_call() {
        std::env::set_var("AUXLOCLAW_APPROVAL_MODE", "smart");
        let orchestrator = ToolOrchestrator::new();
        let result = orchestrator
            .execute_tool(
                "execute_code",
                serde_json::json!({"language": "shell", "code": "rm -rf /"}),
            )
            .await;
        assert!(!result.success);
        assert_eq!(result.output["risk"], "critical");
        assert_eq!(result.output["requires_approval"], false);
        std::env::remove_var("AUXLOCLAW_APPROVAL_MODE");
    }

    #[tokio::test]
    async fn approval_allows_low_risk_tool_call() {
        std::env::set_var("AUXLOCLAW_APPROVAL_MODE", "smart");
        let orchestrator = ToolOrchestrator::new();
        let result = orchestrator
            .execute_tool(
                "execute_code",
                serde_json::json!({"language": "shell", "code": "echo approval-ok"}),
            )
            .await;
        assert!(result.success);
        assert_eq!(result.output["stdout"], "approval-ok\n");
        std::env::remove_var("AUXLOCLAW_APPROVAL_MODE");
    }
}
