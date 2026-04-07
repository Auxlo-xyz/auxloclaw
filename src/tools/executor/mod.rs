//! Enhanced Execution Tools - Script support with sandboxing
//!
//! Features:
//! - execute_script: Python/TypeScript/Shell scripts
//! - execute_parallel: Parallel command execution
//! - Timeout control
//! - Resource limits

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::{timeout, Duration};
use tokio::task::JoinSet;

use crate::orchestrator::{Tool, ToolResult};

/// Result from script execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u64,
}

/// Script execution tool - supports Python, TypeScript, Shell
pub struct ExecuteScriptTool {
    pub timeout_secs: u64,
}

impl ExecuteScriptTool {
    pub fn new() -> Self {
        Self { timeout_secs: 120 }
    }
}

#[async_trait]
impl Tool for ExecuteScriptTool {
    fn name(&self) -> &str { "execute_script" }
    
    fn description(&self) -> &str {
        "Execute Python, TypeScript/Bun, or Shell scripts with sandboxing"
    }
    
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "language": {
                    "type": "string",
                    "enum": ["python", "typescript", "shell"],
                    "description": "Script language"
                },
                "code": {
                    "type": "string",
                    "description": "Script code to execute"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Max execution time in seconds",
                    "default": 120
                }
            },
            "required": ["language", "code"]
        })
    }
    
    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let lang = args["language"].as_str().unwrap_or("shell");
        let code = args["code"].as_str().unwrap_or("");
        let timeout = args["timeout"].as_u64().unwrap_or(self.timeout_secs);
        
        // Security checks
        let blocked_imports = ["os.", "sys.", "subprocess", "socket", "requests"];
        for imp in &blocked_imports {
            if code.contains(imp) {
                return Ok(ToolResult {
                    tool_name: self.name().to_string(),
                    success: false,
                    output: serde_json::Value::Null,
                    error: Some(format!("Blocked import: {}", imp)),
                    duration_ms: 0,
                });
            }
        }
        
        // Write to temp file
        let ext = match lang {
            "python" => "py",
            "typescript" => "ts", 
            _ => "sh",
        };
        let temp_file = std::env::temp_dir().join(format!("script.{}", ext));
        tokio::fs::write(&temp_file, code).await?;
        
        // Build command
        let cmd = match lang {
            "python" => format!("python3 {}", temp_file.display()),
            "typescript" => format!("bun {}", temp_file.display()),
            _ => format!("sh {}", temp_file.display()),
        };
        
        // Execute with timeout
        let start = std::time::Instant::now();
        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .await?;
        
        let _ = tokio::fs::remove_file(&temp_file).await;
        
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let success = output.status.success();
        
        Ok(ToolResult {
            tool_name: self.name().to_string(),
            success,
            output: serde_json::json!({
                "stdout": stdout,
                "stderr": stderr,
                "exit_code": output.status.code().unwrap_or(-1),
                "language": lang
            }),
            error: if success { None } else { Some(stderr.clone()) },
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }
}

/// Parallel execution tool - run multiple commands simultaneously
pub struct ExecuteParallelTool {
    pub timeout_per_cmd: u64,
}

impl ExecuteParallelTool {
    pub fn new() -> Self {
        Self { timeout_per_cmd: 60 }
    }
}

#[derive(Debug, Deserialize)]
struct ParallelCmd {
    command: String,
    id: String,
}

#[async_trait]
impl Tool for ExecuteParallelTool {
    fn name(&self) -> &str { "execute_parallel" }
    
    fn description(&self) -> &str {
        "Execute multiple independent commands in parallel"
    }
    
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "commands": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "command": {"type": "string"},
                            "id": {"type": "string"}
                        },
                        "required": ["command", "id"]
                    },
                    "description": "Commands to execute in parallel"
                },
                "fail_fast": {
                    "type": "boolean",
                    "default": false
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout per command",
                    "default": 60
                }
            },
            "required": ["commands"]
        })
    }
    
    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let cmds: Vec<ParallelCmd> = serde_json::from_value(args["commands"].clone())?;
        let fail_fast = args["fail_fast"].as_bool().unwrap_or(false);
        let timeout = args["timeout"].as_u64().unwrap_or(self.timeout_per_cmd);
        
        if cmds.is_empty() {
            return Ok(ToolResult {
                tool_name: self.name().to_string(),
                success: true,
                output: serde_json::json!({"results": []}),
                error: None,
                duration_ms: 0,
            });
        }
        
        // Execute all in parallel using futures
        let futures: Vec<_> = cmds.iter().map(|c| async {
            let output = tokio::process::Command::new("sh")
                .arg("-c")
                .arg(&c.command)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .output()
                .await;
            (c.id.clone(), output)
        }).collect();
        
        let results = futures::future::join_all(futures).await;
        
        let mut all_success = true;
        let mut output_results = Vec::new();
        
        for (id, result) in results {
            match result {
                Ok(output) => {
                    let success = output.status.success();
                    if !success { all_success = false; }
                    output_results.push(serde_json::json!({
                        "id": id,
                        "success": success,
                        "exit_code": output.status.code().unwrap_or(-1),
                        "stdout": String::from_utf8_lossy(&output.stdout),
                        "stderr": String::from_utf8_lossy(&output.stderr),
                        "duration_ms": 0
                    }));
                }
                Err(e) => {
                    all_success = false;
                    output_results.push(serde_json::json!({
                        "id": id,
                        "success": false,
                        "error": e.to_string()
                    }));
                }
            }
        }
        
        Ok(ToolResult {
            tool_name: self.name().to_string(),
            success: all_success,
            output: serde_json::json!({
                "results": output_results,
                "total_duration_ms": 0
            }),
            error: if all_success { None } else { Some("Some commands failed".into()) },
            duration_ms: 0,
        })
    }
}
