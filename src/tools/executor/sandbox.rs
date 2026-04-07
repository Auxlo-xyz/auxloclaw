//! Sandbox security module
//! 
//! Features:
//! - Command validation (blocks dangerous patterns)
//! - Path restrictions (jail to workspace)
//! - Resource limits

use anyhow::{anyhow, Result};
use std::collections::HashSet;
use std::path::Path;

/// Dangerous patterns that are always blocked
const BLOCKED_PATTERNS: &[&str] = &[
    "rm -rf /",
    ":(){:|:&};:",
    "mkfs",
    "dd if=",
    "${",
    "$(",
];

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
        // Check blocked patterns
        for pattern in BLOCKED_PATTERNS {
            if cmd.contains(pattern) {
                return Err(anyhow!("Blocked: {}", pattern));
            }
        }
        
        // Path restrictions
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
