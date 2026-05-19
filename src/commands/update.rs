//! /update command - pull latest, build, install binary.

use std::process::Command;

const REPO_DIR: &str = "/home/workspace/auxloclaw";
const CARGO_BIN: &str = "/root/.cargo/bin/cargo";
const INSTALL_PATH: &str = "/usr/local/bin/auxloclaw";

/// Run the full update cycle and return a human-readable result string.
pub async fn handle_update() -> String {
    match run_update().await {
        Ok(msg) => msg,
        Err(e) => format!("Update failed: {e}"),
    }
}

async fn run_update() -> Result<String, anyhow::Error> {
    let mut report = String::new();

    // Step 1: git pull
    report.push_str("Pulling latest changes...\n");
    let pull = Command::new("git")
        .args(["-C", REPO_DIR, "pull", "--ff-only"])
        .output()?;

    if !pull.status.success() {
        let stderr = String::from_utf8_lossy(&pull.stderr);
        let stdout = String::from_utf8_lossy(&pull.stdout);
        anyhow::bail!("git pull failed:\n{stdout}\n{stderr}");
    }

    let pull_out = String::from_utf8_lossy(&pull.stdout).trim().to_string();
    if pull_out.contains("Already up to date") {
        report.push_str("Already up to date.\n");
    } else {
        report.push_str(&format!("{pull_out}\n"));
    }

    // Step 2: cargo build --release
    report.push_str("Building release binary...\n");
    let build = Command::new(CARGO_BIN)
        .args(["build", "--release"])
        .current_dir(REPO_DIR)
        .output()?;

    if !build.status.success() {
        let stderr = String::from_utf8_lossy(&build.stderr);
        anyhow::bail!("cargo build failed:\n{stderr}");
    }
    report.push_str("Build succeeded.\n");

    // Step 3: install binary
    let src = format!("{}/target/release/auxloclaw", REPO_DIR);
    report.push_str(&format!("Installing to {INSTALL_PATH}...\n"));
    let install = Command::new("cp")
        .args([&src, INSTALL_PATH])
        .output()?;

    if !install.status.success() {
        let stderr = String::from_utf8_lossy(&install.stderr);
        anyhow::bail!("cp failed:\n{stderr}");
    }

    // chmod +x
    let _ = Command::new("chmod")
        .args(["+x", INSTALL_PATH])
        .output()?;

    // Step 4: verify
    let version = Command::new(INSTALL_PATH)
        .args(["--version"])
        .output()?;

    let ver = String::from_utf8_lossy(&version.stdout).trim().to_string();
    report.push_str(&format!("Installed: {ver}\n"));
    report.push_str("Update complete. Restart the gateway to use the new version.");

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_are_valid_paths() {
        assert!(std::path::Path::new(REPO_DIR).exists());
        assert!(std::path::Path::new(CARGO_BIN).exists());
    }
}
