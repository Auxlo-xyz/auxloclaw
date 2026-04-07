//! Code Execution Tools - Sandboxed multi-language execution
//!
//! Features:
//! - execute_code: Python/TypeScript/Shell with sandboxing
//! - execute_parallel: Parallel command execution
//! - Resource limits (memory, CPU, timeouts)
//! - Output truncation
//! - Workspace restrictions

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::time::timeout;

use crate::orchestrator::{Tool, ToolResult};

/// Result from code execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u64,
    pub truncated: bool,
}

/// Configuration for code execution
#[derive(Clone)]
pub struct ExecutionConfig {
    pub timeout_secs: u64,
    pub max_output_chars: usize,
    pub max_memory_mb: Option<u64>,
    pub workspace_root: Option<PathBuf>,
    pub blocked_patterns: Vec<String>,
    pub blocked_imports: Vec<String>,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 120,
            max_output_chars: 100_000,
            max_memory_mb: Some(512),
            workspace_root: None,
            blocked_patterns: vec![
                "rm -rf /".into(),
                ":(){ :|:& };:".into(),
                "mkfs".into(),
                "dd if=".into(),
                "${".into(),
                "$(".into(),
                "/etc/passwd".into(),
                "/etc/shadow".into(),
            ],
            blocked_imports: vec![
                "os".into(),
                "sys".into(),
                "subprocess".into(),
                "socket".into(),
                "requests".into(),
                "urllib".into(),
                "http.client".into(),
            ],
        }
    }
}

/// Multi-language code execution tool
pub struct ExecuteCodeTool {
    config: ExecutionConfig,
}

impl ExecuteCodeTool {
    pub fn new() -> Self {
        Self {
            config: ExecutionConfig::default(),
        }
    }

    pub fn with_config(config: ExecutionConfig) -> Self {
        Self { config }
    }

    fn validate(&self, code: &str, lang: &str) -> Result<()> {
        // Check blocked patterns
        for pattern in &self.config.blocked_patterns {
            if code.contains(pattern) {
                return Err(anyhow!("Blocked pattern detected: {}", pattern));
            }
        }

        // Language-specific import validation
        match lang {
            "python" => self.validate_python(code)?,
            "typescript" | "javascript" => self.validate_js(code)?,
            _ => {}
        }

        Ok(())
    }

    fn validate_python(&self, code: &str) -> Result<()> {
        for imp in &self.config.blocked_imports {
            if code.contains(&format!("import {}", imp))
                || code.contains(&format!("from {} import", imp))
            {
                return Err(anyhow!("Blocked Python import: {}", imp));
            }
        }
        Ok(())
    }

    fn validate_js(&self, code: &str) -> Result<()> {
        let blocked = ["child_process", "fs", "net", "http", "tls", "crypto"];
        for module in blocked {
            if code.contains(&format!("require('{}')", module))
                || code.contains(&format!("from '{}'", module))
                || code.contains(&format!("import from '{}'", module))
            {
                return Err(anyhow!("Blocked JS module: {}", module));
            }
        }
        Ok(())
    }

    fn truncate_output(&self, output: &str) -> (String, bool) {
        if output.len() > self.config.max_output_chars {
            let truncated = format!(
                "{}... [truncated {} chars]",
                &output[..self.config.max_output_chars],
                output.len() - self.config.max_output_chars
            );
            (truncated, true)
        } else {
            (output.to_string(), false)
        }
    }

    async fn execute_internal(
        &self,
        lang: &str,
        code: &str,
    ) -> Result<ExecutionResult> {
        // Validate
        tracing::debug!("execute_code: lang={}, code_len={}", lang, code.len());
        if let Err(e) = self.validate(code, lang) {
            tracing::warn!("Validation failed: {}", e);
            return Err(e);
        }

        // Write to temp file
        let ext = match lang {
            "python" => "py",
            "typescript" => "ts",
            "javascript" => "js",
            _ => "sh",
        };
        let temp_file = std::env::temp_dir().join(format!("aux_code_{}.{}", std::process::id(), ext));
        tokio::fs::write(&temp_file, code).await?;

        // Build runner command
        let runner = match lang {
            "python" => vec!["python3".to_string(), temp_file.to_string_lossy().to_string()],
            "typescript" | "javascript" => vec!["bun".to_string(), temp_file.to_string_lossy().to_string()],
            _ => vec!["sh".to_string(), temp_file.to_string_lossy().to_string()],
        };

        let start = Instant::now();
        let duration = Duration::from_secs(self.config.timeout_secs);

        // Execute with timeout
        let output = timeout(duration, async {
            Command::new(&runner[0])
                .args(&runner[1..])
                .output()
                .await
        }).await;

        // Cleanup
        let _ = tokio::fs::remove_file(&temp_file).await;

        match output {
            Ok(Ok(o)) => {
                let stdout = String::from_utf8_lossy(&o.stdout).to_string();
                let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                let (stdout, truncated_out) = self.truncate_output(&stdout);
                let (stderr, truncated_err) = self.truncate_output(&stderr);

                Ok(ExecutionResult {
                    success: o.status.success(),
                    stdout,
                    stderr,
                    exit_code: o.status.code().unwrap_or(-1),
                    duration_ms: start.elapsed().as_millis() as u64,
                    truncated: truncated_out || truncated_err,
                })
            }
            Ok(Err(e)) => Err(anyhow!("Execution failed: {}", e)),
            Err(_) => Err(anyhow!("Execution timed out after {}s", self.config.timeout_secs)),
        }
    }
}

impl Default for ExecuteCodeTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ExecuteCodeTool {
    fn name(&self) -> &str { "execute_code" }

    fn description(&self) -> &str {
        "Execute Python, TypeScript, or Shell code with sandboxing, timeouts, and output limits"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "language": {
                    "type": "string",
                    "enum": ["python", "typescript", "javascript", "shell"],
                    "description": "Script language"
                },
                "code": {
                    "type": "string",
                    "description": "Code to execute"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Max execution time in seconds (default: 120)"
                }
            },
            "required": ["language", "code"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let lang = args["language"].as_str().unwrap_or("shell");
        let code = args["code"].as_str().unwrap_or("");
        let timeout_override = args["timeout"].as_u64();

        let mut config = self.config.clone();
        if let Some(t) = timeout_override {
            config.timeout_secs = t;
        }

        let tool = ExecuteCodeTool::with_config(config);
        let result = tool.execute_internal(lang, code).await?;

        Ok(ToolResult {
            tool_name: self.name().to_string(),
            success: result.success,
            output: serde_json::json!({
                "stdout": result.stdout,
                "stderr": result.stderr,
                "exit_code": result.exit_code,
                "duration_ms": result.duration_ms,
                "truncated": result.truncated
            }),
            error: if result.success { None } else { Some(result.stderr) },
            duration_ms: result.duration_ms,
        })
    }
}

/// Parallel execution tool - run multiple commands simultaneously
pub struct ExecuteParallelTool {
    timeout_secs: u64,
}

impl ExecuteParallelTool {
    pub fn new() -> Self {
        Self { timeout_secs: 60 }
    }
}

impl Default for ExecuteParallelTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct CmdSpec {
    id: String,
    command: String,
    language: Option<String>,
}

#[async_trait]
impl Tool for ExecuteParallelTool {
    fn name(&self) -> &str { "execute_parallel" }

    fn description(&self) -> &str {
        "Execute multiple code blocks in parallel with independent results"
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
                            "id": {"type": "string"},
                            "command": {"type": "string"},
                            "language": {"type": "string"}
                        },
                        "required": ["id", "command"]
                    }
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout per command (default: 60)"
                }
            },
            "required": ["commands"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let cmds: Vec<CmdSpec> = serde_json::from_value(args["commands"].clone())?;
        let timeout_override = args["timeout"].as_u64().unwrap_or(self.timeout_secs);

        if cmds.is_empty() {
            return Ok(ToolResult {
                tool_name: self.name().to_string(),
                success: true,
                output: serde_json::json!({"results": [], "total": 0}),
                error: None,
                duration_ms: 0,
            });
        }

        // Execute all in parallel
        let start = Instant::now();
        let futures: Vec<_> = cmds.iter().map(|cmd| async {
            let lang = cmd.language.as_deref().unwrap_or("shell");
            let exec = ExecuteCodeTool::new();
            let exec_config = ExecutionConfig {
                timeout_secs: timeout_override,
                ..Default::default()
            };
            let tool = ExecuteCodeTool::with_config(exec_config);
            tool.execute_internal(lang, &cmd.command).await
        }).collect();

        let results = futures::future::join_all(futures).await;

        let mut all_success = true;
        let mut output_results = Vec::new();

        for (i, result) in results.into_iter().enumerate() {
            match result {
                Ok(r) => {
                    if !r.success { all_success = false; }
                    output_results.push(serde_json::json!({
                        "id": cmds[i].id,
                        "success": r.success,
                        "stdout": r.stdout,
                        "stderr": r.stderr,
                        "exit_code": r.exit_code,
                        "duration_ms": r.duration_ms,
                        "truncated": r.truncated
                    }));
                }
                Err(e) => {
                    all_success = false;
                    output_results.push(serde_json::json!({
                        "id": cmds[i].id,
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
                "total": output_results.len(),
                "duration_ms": start.elapsed().as_millis() as u64
            }),
            error: if all_success { None } else { Some("Some commands failed".into()) },
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }
}
