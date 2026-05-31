//! Sandbox configuration and enforcement.
//!
//! Provides resource limits, blocked patterns, and workspace restrictions
//! that apply across all execution environments. This is the safety layer
//! that prevents destructive operations regardless of backend.

use anyhow::{anyhow, Result};
use std::collections::HashSet;
use std::path::PathBuf;

/// Dangerous patterns that are always blocked
const BLOCKED_PATTERNS: &[&str] = &[
    "rm -rf /",
    "(){:|:&};:",
    "mkfs",
    "dd if=",
];

/// Legacy sandbox (kept for backward compatibility).
pub struct Sandbox {
    workspace: Option<String>,
    allowlist: HashSet<String>,
    restrict: bool,
}

impl Sandbox {
    pub fn new() -> Self {
        Self {
            workspace: None,
            allowlist: HashSet::new(),
            restrict: true,
        }
    }

    pub fn with_workspace(mut self, ws: &str) -> Self {
        self.workspace = Some(ws.to_string());
        self
    }

    pub fn allow(&mut self, cmd: &str) {
        self.allowlist.insert(cmd.to_string());
    }

    /// Validate command for safety
    pub fn validate(&self, cmd: &str) -> Result<()> {
        for pattern in BLOCKED_PATTERNS {
            if cmd.contains(pattern) {
                return Err(anyhow!("Blocked: {}", pattern));
            }
        }

        if self.restrict {
            if cmd.contains("/root") && !cmd.contains(self.workspace.as_deref().unwrap_or("")) {
                return Err(anyhow!("Access denied to /root"));
            }
            if cmd.contains("/etc/passwd") {
                return Err(anyhow!("Access denied to /etc/passwd"));
            }
        }

        Ok(())
    }
}

impl Default for Sandbox {
    fn default() -> Self {
        Self::new()
    }
}

/// Resource limits for execution
pub struct Limits {
    pub max_memory_mb: Option<u64>,
    pub max_cpu_secs: Option<u64>,
    pub max_output_kb: Option<u64>,
}

impl Limits {
    pub fn default_limits() -> Self {
        Self {
            max_memory_mb: Some(512),
            max_cpu_secs: Some(60),
            max_output_kb: Some(1024),
        }
    }
}

/// Enhanced sandbox configuration with environment-aware validation.
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    pub max_output_chars: usize,
    pub max_memory_mb: Option<u64>,
    pub workspace_root: Option<PathBuf>,
    pub blocked_patterns: Vec<String>,
    pub blocked_imports: Vec<String>,
    pub blocked_modules: Vec<String>,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            max_output_chars: 100_000,
            max_memory_mb: Some(512),
            workspace_root: None,
            blocked_patterns: vec![
                "rm -rf /".into(),
                "(){:|:&};:".into(),
                "mkfs".into(),
                "dd if=".into(),
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
            blocked_modules: vec![
                "child_process".into(),
                "fs".into(),
                "net".into(),
            ],
        }
    }
}

impl SandboxConfig {
    pub fn validate_command(&self, code: &str) -> Result<(), String> {
        for pattern in &self.blocked_patterns {
            if code.contains(pattern) {
                return Err(format!("Blocked pattern detected: {}", pattern));
            }
        }
        Ok(())
    }

    pub fn validate_python(&self, code: &str) -> Result<(), String> {
        for imp in &self.blocked_imports {
            if code.contains(&format!("import {}", imp))
                || code.contains(&format!("from {} import", imp))
            {
                return Err(format!("Blocked Python import: {}", imp));
            }
        }
        Ok(())
    }

    pub fn validate_js(&self, code: &str) -> Result<(), String> {
        for module in &self.blocked_modules {
            if code.contains(&format!("require('{}')", module))
                || code.contains(&format!("from '{}'", module))
                || code.contains(&format!("import from '{}'", module))
            {
                return Err(format!("Blocked JS module: {}", module));
            }
        }
        Ok(())
    }

    pub fn truncate_output(&self, output: &str) -> (String, bool) {
        if output.len() > self.max_output_chars {
            let truncated = format!(
                "{}... [truncated {} chars]",
                &output[..self.max_output_chars],
                output.len() - self.max_output_chars
            );
            (truncated, true)
        } else {
            (output.to_string(), false)
        }
    }

    pub fn is_within_workspace(&self, path: &PathBuf) -> bool {
        if let Some(ref root) = self.workspace_root {
            path.starts_with(root)
        } else {
            true
        }
    }
}
