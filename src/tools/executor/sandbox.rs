//! Sandbox configuration and enforcement.
//!
//! Provides resource limits, blocked patterns, and workspace restrictions
//! that apply across all execution environments. This is the safety layer
//! that prevents destructive operations regardless of backend.

use std::path::PathBuf;

/// Sandbox configuration applied to all code execution.
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Maximum output size in characters.
    pub max_output_chars: usize,
    /// Maximum memory in MB (None = no limit).
    pub max_memory_mb: Option<u64>,
    /// Workspace root directory (code can only access this and children).
    pub workspace_root: Option<PathBuf>,
    /// Patterns that are blocked in any command.
    pub blocked_patterns: Vec<String>,
    /// Python imports that are blocked.
    pub blocked_imports: Vec<String>,
    /// JS/TS modules that are blocked.
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
                ":(){ :|:& };:".into(),
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
    /// Validate that a command doesn't contain blocked patterns.
    pub fn validate_command(&self, code: &str) -> Result<(), String> {
        for pattern in &self.blocked_patterns {
            if code.contains(pattern) {
                return Err(format!("Blocked pattern detected: {}", pattern));
            }
        }
        Ok(())
    }

    /// Validate Python code for blocked imports.
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

    /// Validate JavaScript/TypeScript code for blocked modules.
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

    /// Truncate output to max_output_chars, returning (output, was_truncated).
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

    /// Check if a path is within the workspace root.
    pub fn is_within_workspace(&self, path: &PathBuf) -> bool {
        if let Some(ref root) = self.workspace_root {
            path.starts_with(root)
        } else {
            true // No workspace restriction
        }
    }
}
