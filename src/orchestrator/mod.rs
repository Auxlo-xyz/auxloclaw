//! Tool Orchestrator with DAG-based Parallel Execution
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Tool trait
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;
    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult>;
    fn dependencies(&self) -> Vec<String> { vec![] }
}

/// Tool result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_name: String,
    pub success: bool,
    pub output: serde_json::Value,
    pub error: Option<String>,
    pub duration_ms: u64,
}

/// Tool definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// DAG node
#[derive(Debug, Clone)]
pub struct ToolNode {
    pub name: String,
    pub args: serde_json::Value,
    pub dependencies: HashSet<String>,
    pub level: usize,
}

/// Tool orchestrator
pub struct ToolOrchestrator {
    registry: DashMap<String, Arc<dyn Tool>>,
}

impl ToolOrchestrator {
    pub fn new() -> Self {
        let orchestrator = Self {
            registry: DashMap::new(),
        };
        
        // Register built-in tools
        orchestrator.register_builtin_tools();
        
        orchestrator
    }
    
    fn register_builtin_tools(&self) {
        self.register(Arc::new(FileReadTool));
        self.register(Arc::new(FileWriteTool));
        self.register(Arc::new(ExecTool));
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

    pub async fn execute_parallel(&self, calls: Vec<(String, serde_json::Value)>) -> Vec<ToolResult> {
        let dag = self.build_dag(&calls);
        let mut all_results = vec![];
        
        for level in dag.levels() {
            let level_nodes = dag.nodes_at_level(level);
            let level_results: Vec<ToolResult> = futures::future::join_all(
                level_nodes.into_iter().map(|node| self.execute_tool(node))
            ).await;
            all_results.extend(level_results);
        }
        all_results
    }

    async fn execute_tool(&self, node: ToolNode) -> ToolResult {
        let start = std::time::Instant::now();
        
        let result = if let Some(tool) = self.registry.get(&node.name) {
            tool.execute(node.args).await
        } else {
            Err(anyhow!("Tool not found: {}", node.name))
        };

        let success = result.is_ok();
        let error = result.as_ref().err().map(|e| e.to_string());
        let output = result.map(|r| r.output).unwrap_or(serde_json::Value::Null);

        ToolResult {
            tool_name: node.name.clone(),
            success,
            output,
            error,
            duration_ms: start.elapsed().as_millis() as u64,
        }
    }

    fn build_dag(&self, calls: &[(String, serde_json::Value)]) -> ExecutionDAG {
        let mut dag = ExecutionDAG::new();
        for (name, args) in calls {
            dag.add_node(ToolNode {
                name: name.clone(),
                args: args.clone(),
                dependencies: HashSet::new(),
                level: 0,
            });
        }
        dag.compute_levels();
        dag
    }
}

/// Execution DAG
pub struct ExecutionDAG {
    nodes: HashMap<String, ToolNode>,
}

impl ExecutionDAG {
    fn new() -> Self {
        Self { nodes: HashMap::new() }
    }

    fn add_node(&mut self, node: ToolNode) {
        let name = node.name.clone();
        self.nodes.insert(name, node);
    }

    fn compute_levels(&mut self) {
        let names: Vec<String> = self.nodes.keys().cloned().collect();
        let mut levels: HashMap<String, usize> = HashMap::new();
        
        for name in &names {
            let level = self.compute_level_recursive(name, &mut levels);
            if let Some(node) = self.nodes.get_mut(name) {
                node.level = level;
            }
        }
    }

    fn compute_level_recursive(&self, name: &str, levels: &mut HashMap<String, usize>) -> usize {
        if let Some(&level) = levels.get(name) {
            return level;
        }
        let deps = self.nodes.get(name).map(|n| n.dependencies.clone()).unwrap_or_default();
        let max_dep = deps.iter().map(|d| self.compute_level_recursive(d, levels)).max().unwrap_or(0);
        let level = max_dep + 1;
        levels.insert(name.to_string(), level);
        level
    }

    fn levels(&self) -> Vec<usize> {
        let mut levels: Vec<usize> = self.nodes.values().map(|n| n.level).collect();
        levels.sort();
        levels.dedup();
        levels
    }

    fn nodes_at_level(&self, level: usize) -> Vec<ToolNode> {
        self.nodes.values().filter(|n| n.level == level).cloned().collect()
    }
}

// Built-in tools

pub struct FileReadTool;
#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str { "file_read" }
    fn description(&self) -> &str { "Read a file" }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]})
    }
    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let path = args["path"].as_str().ok_or_else(|| anyhow!("Missing path"))?;
        let content = tokio::fs::read_to_string(path).await?;
        Ok(ToolResult {
            tool_name: self.name().into(),
            success: true,
            output: serde_json::json!({"content": content}),
            error: None,
            duration_ms: 0,
        })
    }
}

pub struct FileWriteTool;
#[async_trait]
impl Tool for FileWriteTool {
    fn name(&self) -> &str { "file_write" }
    fn description(&self) -> &str { "Write a file" }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}, "content": {"type": "string"}}, "required": ["path", "content"]})
    }
    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let path = args["path"].as_str().ok_or_else(|| anyhow!("Missing path"))?;
        let content = args["content"].as_str().ok_or_else(|| anyhow!("Missing content"))?;
        tokio::fs::write(path, content).await?;
        Ok(ToolResult {
            tool_name: self.name().into(),
            success: true,
            output: serde_json::json!({"written": true}),
            error: None,
            duration_ms: 0,
        })
    }
}

pub struct ExecTool;
#[async_trait]
impl Tool for ExecTool {
    fn name(&self) -> &str { "execute" }
    fn description(&self) -> &str { "Execute a shell command" }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {"command": {"type": "string"}}, "required": ["command"]})
    }
    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let cmd = args["command"].as_str().ok_or_else(|| anyhow!("Missing command"))?;
        let output = tokio::process::Command::new("sh").arg("-c").arg(cmd).output().await?;
        Ok(ToolResult {
            tool_name: self.name().into(),
            success: output.status.success(),
            output: serde_json::json!({"stdout": String::from_utf8_lossy(&output.stdout), "stderr": String::from_utf8_lossy(&output.stderr)}),
            error: None,
            duration_ms: 0,
        })
    }
}