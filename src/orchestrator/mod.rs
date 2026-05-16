//! Tool Orchestrator
use anyhow::Result;
use async_trait::async_trait;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::plugins::{plugin_result_error, SharedPluginManager};

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;
    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult>;
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolResult {
    pub tool_name: String,
    pub success: bool,
    pub output: serde_json::Value,
    pub error: Option<String>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub tool_type: String,
    pub function: FunctionDef,
}

#[derive(Debug, Clone, Serialize)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

pub struct ToolOrchestrator {
    registry: DashMap<String, Arc<dyn Tool>>,
    approval_policy: crate::tools::approval::ApprovalPolicy,
    plugins: Option<SharedPluginManager>,
}

impl ToolOrchestrator {
    pub fn new() -> Self {
        let orch = Self {
            registry: DashMap::new(),
            approval_policy: crate::tools::approval::ApprovalPolicy::from_env(),
            plugins: None,
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

    pub async fn register_mcp_tools(
        &self,
        config: &crate::config::McpConfig,
    ) -> anyhow::Result<usize> {
        let registry = crate::mcp::McpRegistry::new();
        let tools = registry.load_tools(config).await?;
        let count = tools.len();
        for tool in tools {
            self.register(tool);
        }
        Ok(count)
    }

    pub fn set_plugins(&mut self, plugins: SharedPluginManager) {
        self.plugins = Some(plugins);
    }

    pub fn register(&self, tool: Arc<dyn Tool>) {
        self.registry.insert(tool.name().to_string(), tool);
    }

    pub fn list_tools(&self) -> Vec<ToolSpec> {
        self.registry
            .iter()
            .map(|t| ToolSpec {
                name: t.name().to_string(),
                description: t.description().to_string(),
                parameters: t.parameters(),
            })
            .collect()
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

    pub async fn execute_tool(&self, name: &str, mut args: serde_json::Value) -> ToolResult {
        let start = std::time::Instant::now();

        if let Some(plugins) = &self.plugins {
            match plugins.process_before_tool(name, args.clone()).await {
                Ok((true, _)) => {
                    return plugin_result_error(name, format!("Tool {} cancelled by plugin", name));
                }
                Ok((false, new_args)) => args = new_args,
                Err(err) => tracing::warn!("before_tool plugin hook failed for {}: {}", name, err),
            }
        }

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
        let tool_result = ToolResult {
            tool_name: name.into(),
            success: result.is_ok(),
            output: result
                .as_ref()
                .ok()
                .map(|r| r.output.clone())
                .unwrap_or(serde_json::Value::Null),
            error: result.as_ref().err().map(|e| e.to_string()),
            duration_ms: start.elapsed().as_millis() as u64,
        };

        if let Some(plugins) = &self.plugins {
            plugins.run_after_tool(name, tool_result.clone()).await;
        }

        tool_result
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
