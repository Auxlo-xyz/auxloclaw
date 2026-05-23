//! /update command - download latest pre-built binary from GitHub releases.

use std::process::Command;

const REPO: &str = "Auxlo-xyz/auxloclaw";
const INSTALL_PATH: &str = "/usr/local/bin/auxloclaw";

/// Run the update cycle and return a human-readable result string.
pub async fn handle_update() -> String {
    match run_update().await {
        Ok(msg) => msg,
        Err(e) => format!("Update failed: {e}"),
    }
}

async fn run_update() -> Result<String, anyhow::Error> {
    let mut report = String::new();

    // Step 1: get current version
    let current = Command::new(INSTALL_PATH)
        .args(["--version"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".into());

    report.push_str(&format!("Current: {current}\n"));

    // Step 2: fetch latest release tag from GitHub API
    report.push_str("Checking for updates...\n");
    let client = reqwest::Client::builder()
        .user_agent("auxloclaw-updater")
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let api_url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let resp = client.get(&api_url).send().await?;

    if !resp.status().is_success() {
        anyhow::bail!("GitHub API returned {}", resp.status());
    }

    let release: serde_json::Value = resp.json().await?;
    let tag = release["tag_name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No tag_name in release"))?;

    let latest_ver = tag.trim_start_matches('v');

    // Step 3: compare versions
    let current_ver = current
        .split_whitespace()
        .nth(1)
        .unwrap_or("0.0.0");

    if current_ver == latest_ver {
        report.push_str(&format!(
            "Already on the latest version (v{current_ver})."
        ));
        return Ok(report);
    }

    report.push_str(&format!(
        "Update available: v{current_ver} -> v{latest_ver}\n"
    ));

    // Step 4: detect platform and find the right asset
    let target = detect_target()?;
    let asset_name = format!("auxloclaw-{target}");

    let assets = release["assets"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("No assets in release"))?;

    let asset = assets
        .iter()
        .find(|a| a["name"].as_str() == Some(&asset_name))
        .ok_or_else(|| {
            anyhow::anyhow!("No binary found for {target}")
        })?;

    let download_url = asset["browser_download_url"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No download URL"))?;

    // Step 5: download to temp file
    report.push_str(&format!("Downloading {asset_name}...\n"));
    let tmp_path = format!("{INSTALL_PATH}.tmp");

    let resp = client.get(download_url).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("Download failed: HTTP {}", resp.status());
    }

    let bytes = resp.bytes().await?;
    std::fs::write(&tmp_path, &bytes)?;

    // Step 6: replace binary
    report.push_str(&format!("Installing to {INSTALL_PATH}...\n"));

    let mv = Command::new("mv")
        .args([&tmp_path, INSTALL_PATH])
        .output()?;

    if !mv.status.success() {
        let _ = std::fs::remove_file(&tmp_path);
        anyhow::bail!(
            "Failed to replace binary: {}",
            String::from_utf8_lossy(&mv.stderr)
        );
    }

    let _ = Command::new("chmod")
        .args(["+x", INSTALL_PATH])
        .output()?;

    // Step 7: verify
    let version = Command::new(INSTALL_PATH)
        .args(["--version"])
        .output()?;

    let ver = String::from_utf8_lossy(&version.stdout).trim().to_string();
    report.push_str(&format!("Installed: {ver}\n"));

    // Step 8: auto-restart gateway
    // Spawn a detached shell that sleeps 5s (giving time for the response
    // to reach the user), then exec-replaces itself with the new binary.
    // exec() replaces the PID so the old gateway stops and the new one starts.
    report.push_str("Restarting gateway in 5 seconds...\n");

    let restart_script = format!(
        "#!/bin/sh\nsleep 5\npkill -f 'auxloclaw gateway'\nsleep 1\nexec {} gateway\n",
        INSTALL_PATH
    );
    let restart_path = "/tmp/auxloclaw-restart.sh";
    std::fs::write(restart_path, restart_script)?;
    let _ = Command::new("chmod").args(["+x", restart_path]).output()?;

    let _ = Command::new("/bin/sh")
        .arg(restart_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;

    Ok(report)
}

fn detect_target() -> Result<String, anyhow::Error> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    match (os, arch) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-musl".into()),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-musl".into()),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin".into()),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin".into()),
        _ => anyhow::bail!("Unsupported platform: {os}/{arch}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_target_works() {
        // Should not panic on the current platform
        let _ = detect_target();
    }
}
