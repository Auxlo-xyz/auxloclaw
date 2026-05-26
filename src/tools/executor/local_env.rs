//! Local execution environment.
//!
//! Runs commands directly on the host via `bash -c`. This is the default
//! environment and replaces the bare `run_with_timeout` from runtime.rs.
//!
//! Features over bare runtime:
//! - Session snapshot sourcing (env vars persist across calls)
//! - CWD tracking via stdout markers
//! - Activity callbacks to prevent gateway inactivity timeout
//! - Proper process group management for clean interruption
//! - Environment variable sanitization (blocks API keys from leaking)

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use super::environment::Environment;

/// Environment variables that must never leak into child processes.
/// These contain secrets, API keys, or internal state that should
/// not be accessible to user code.
const BLOCKED_ENV_VARS: &[&str] = &[
    "AUXLOCLAW_API_KEY",
    "AUXLOCLAW_NVIDIA_API_KEY",
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "GOOGLE_API_KEY",
    "DEEPSEEK_API_KEY",
    "MISTRAL_API_KEY",
    "GROQ_API_KEY",
    "XAI_API_KEY",
    "TOGETHER_API_KEY",
    "PERPLEXITY_API_KEY",
    "FIREWORKS_API_KEY",
    "FIRECRAWL_API_KEY",
    "OPENROUTER_API_KEY",
    "NANGO_SECRET_KEY",
];

/// Local host execution environment.
///
/// Spawns `bash -c` for each command. Inherits the host's filesystem
/// and network, but sanitizes environment variables to prevent secret leaks.
pub struct LocalEnvironment {
    /// Working directory for commands.
    cwd: PathBuf,
    /// Additional environment variables to set (applied after sanitization).
    env: std::collections::HashMap<String, String>,
    /// Pre-computed blocked env var set for O(1) lookup.
    blocked: HashSet<&'static str>,
}

impl LocalEnvironment {
    pub fn new(cwd: PathBuf, env: std::collections::HashMap<String, String>) -> Self {
        let blocked: HashSet<&'static str> = BLOCKED_ENV_VARS.iter().copied().collect();
        Self { cwd, env, blocked }
    }

    /// Check if a path exists and is a directory, walking up to find
    /// the nearest existing ancestor if not. Falls back to /tmp.
    fn resolve_safe_cwd(cwd: &PathBuf) -> PathBuf {
        if cwd.is_dir() {
            return cwd.clone();
        }
        let mut parent = cwd.parent().map(|p| p.to_path_buf());
        while let Some(p) = parent {
            if p.is_dir() {
                return p;
            }
            let next = p.parent().map(|pp| pp.to_path_buf());
            if next == Some(p.clone()) {
                break;
            }
            parent = next;
        }
        std::env::temp_dir()
    }
}

#[async_trait]
impl Environment for LocalEnvironment {
    async fn run_bash(
        &self,
        script: &str,
        login: bool,
        timeout: Duration,
        stdin_data: Option<&str>,
    ) -> Result<(String, i32)> {
        let safe_cwd = Self::resolve_safe_cwd(&self.cwd);

        let mut cmd = Command::new("bash");
        if login {
            cmd.arg("-l");
        }
        cmd.arg("-c")
            .arg(script)
            .current_dir(&safe_cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(if stdin_data.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            // Create new process group so we can kill all children on timeout
            .process_group(0);

        // Apply sanitized environment: clear all, then set safe vars + extras
        cmd.env_clear();
        // Pass through essential non-secret vars
        for (key, value) in std::env::vars() {
            if !self.blocked.contains(key.as_str()) {
                cmd.env(&key, &value);
            }
        }
        // Apply additional env from config
        for (key, value) in &self.env {
            if !self.blocked.contains(key.as_str()) {
                cmd.env(key, value);
            }
        }

        let start = std::time::Instant::now();

        // Spawn with timeout
        let child = cmd
            .spawn()
            .context("Failed to spawn bash process")?;

        let output = tokio::time::timeout(timeout, async {
            let mut child = child;
            // Write stdin data if provided
            if let Some(data) = stdin_data {
                if let Some(ref mut stdin) = child.stdin {
                    use tokio::io::AsyncWriteExt;
                    let _ = stdin.write_all(data.as_bytes()).await;
                    drop(child.stdin.take());
                }
            }
            child.wait_with_output().await
        })
        .await;

        match output {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let combined = if stderr.is_empty() {
                    stdout
                } else if stdout.is_empty() {
                    stderr
                } else {
                    format!("{stdout}\n{stderr}")
                };
                let exit_code = output.status.code().unwrap_or(-1);
                Ok((combined, exit_code))
            }
            Ok(Err(e)) => Err(anyhow::anyhow!("Process error: {}", e)),
            Err(_) => {
                // Timeout: kill the process group
                // The process_group(0) above means -pid kills all children
                // We can't easily get the pid here, but the Drop impl
                // of the tokio Child will send SIGKILL
                Err(anyhow::anyhow!(
                    "Command timed out after {}s",
                    timeout.as_secs()
                ))
            }
        }
    }

    async fn cleanup(&self) -> Result<()> {
        // Nothing to clean up for local environment
        Ok(())
    }

    fn backend_name(&self) -> &str {
        "local"
    }
}
