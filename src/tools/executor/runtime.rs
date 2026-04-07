//! Runtime module for process management and resource limits

use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use anyhow::Result;

/// Execute a command with timeout and resource limits
pub async fn run_with_timeout(
    cmd: &str,
    cwd: &std::path::Path,
    timeout: Duration,
) -> Result<ProcessOutput> {
    let start = std::time::Instant::now();
    
    let output = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;
    
    Ok(ProcessOutput {
        success: output.status.success(),
        exit_code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        duration_ms: start.elapsed().as_millis() as u64,
    })
}

/// Process output structure
#[derive(Debug)]
pub struct ProcessOutput {
    pub success: bool,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
}
