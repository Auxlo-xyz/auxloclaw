//! /token command - Secure token/secret management for MCP and providers
//!
//! Usage:
//!   /token                    - List all configured tokens (masked)
//!   /token set <server> <KEY> <value>  - Set a token for an MCP server
//!   /token set <KEY> <value>           - Set a global env token
//!   /token remove <server> <KEY>       - Remove a token from an MCP server
//!   /token remove <KEY>                - Remove a global env token
//!   /token help                       - Show usage
//!
//! Security: Messages containing tokens are auto-deleted from Telegram chats.

use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::config::AppConfig;

fn config_path() -> PathBuf {
    let base = std::env::var("AUXLOCLAW_CONFIG")
        .unwrap_or_else(|_| "~/.auxloclaw/config.toml".into());
    if base.starts_with('~') {
        dirs::home_dir()
            .unwrap_or_else(|| "/root".into())
            .join(&base[2..])
    } else {
        PathBuf::from(&base)
    }
}

fn load_config(path: &PathBuf) -> Result<AppConfig> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config: {:?}", path))?;
    toml::from_str(&content).context("Failed to parse config")
}

fn save_config(path: &PathBuf, config: &AppConfig) -> Result<()> {
    let content = toml::to_string_pretty(config)
        .context("Failed to serialize config")?;
    let tmp = path.with_extension("toml.tmp");
    fs::write(&tmp, &content)
        .with_context(|| format!("Failed to write config tmp: {:?}", tmp))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("Failed to rename: {:?} -> {:?}", tmp, path))?;
    Ok(())
}

/// Mask a secret value for display: show first 4 and last 4 chars
fn mask_secret(value: &str) -> String {
    if value.len() <= 12 {
        if value.is_empty() {
            "(not set)".to_string()
        } else {
            "*".repeat(value.len())
        }
    } else {
        format!("{}...{}", &value[..4], &value[value.len()-4..])
    }
}

/// Detect if a message contains what looks like a token/secret
pub fn contains_secret(text: &str) -> bool {
    let lower = text.to_lowercase();
    let patterns = [
        "ghp_", "gho_", "ghs_", "ghr_",           // GitHub tokens
        "sk-", "sk_live_", "sk_test_",              // Stripe/OpenAI
        "xoxb-", "xoxp-", "xapp-",                  // Slack
        "nvapi-",                                    // Nvidia
        "Bearer ", "bearer ",                         // Auth headers
        "api_key=", "apikey=", "token=",             // Inline keys
        "pat-",                                      // Azure DevOps
        "AKIA",                                      // AWS
        "eyJ",                                       // JWT tokens
        "whsec_",                                    // Stripe webhooks
    ];
    for pat in &patterns {
        if lower.contains(&pat.to_lowercase()) {
            return true;
        }
    }
    // Also flag long alphanumeric strings (>40 chars) that look like keys
    for word in text.split_whitespace() {
        let clean: String = word.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
        if clean.len() > 40 && clean.chars().filter(|c| c.is_ascii_alphanumeric()).count() > 35 {
            return true;
        }
    }
    false
}

/// Handle the /token command
pub fn handle_token(args: &str) -> Result<String> {
    let parts: Vec<&str> = args.trim().split_whitespace().collect();

    if parts.is_empty() || parts[0] == "list" {
        return list_tokens();
    }

    match parts[0] {
        "help" | "h" => Ok(help_text()),
        "set" => {
            // /token set <server> <KEY> <value>
            // /token set <KEY> <value>
            if parts.len() < 3 {
                return Err(anyhow!(
                    "Usage:\n  /token set <server> <KEY> <value>\n  /token set <KEY> <value>"
                ));
            }

            // Check if 2nd arg is an MCP server name
            let path = config_path();
            let config = load_config(&path)?;
            let server_names: Vec<&str> = config.mcp.servers.iter().map(|s| s.name.as_str()).collect();

            if parts.len() >= 4 && server_names.contains(&parts[1]) {
                // /token set <server> <KEY> <value>
                set_server_token(&path, parts[1], parts[2], &parts[3..].join(" "))
            } else {
                // /token set <KEY> <value>
                // Default to first MCP server if only one exists, or error
                if server_names.len() == 1 {
                    set_server_token(&path, server_names[0], parts[1], &parts[2..].join(" "))
                } else if server_names.is_empty() {
                    Err(anyhow!("No MCP servers configured. Add one first with /mcp add"))
                } else {
                    Err(anyhow!(
                        "Multiple MCP servers: {}. Specify which one:\n  /token set <server> <KEY> <value>",
                        server_names.join(", ")
                    ))
                }
            }
        }
        "remove" | "rm" => {
            if parts.len() < 2 {
                return Err(anyhow!(
                    "Usage:\n  /token remove <server> <KEY>\n  /token remove <KEY>"
                ));
            }

            let path = config_path();
            let config = load_config(&path)?;
            let server_names: Vec<&str> = config.mcp.servers.iter().map(|s| s.name.as_str()).collect();

            if parts.len() >= 3 && server_names.contains(&parts[1]) {
                remove_server_token(&path, parts[1], parts[2])
            } else if server_names.len() == 1 {
                remove_server_token(&path, server_names[0], parts[1])
            } else if server_names.is_empty() {
                Err(anyhow!("No MCP servers configured."))
            } else {
                Err(anyhow!(
                    "Multiple MCP servers: {}. Specify:\n  /token remove <server> <KEY>",
                    server_names.join(", ")
                ))
            }
        }
        _ => Err(anyhow!(
            "Unknown subcommand '{}'. Use /token help for usage.",
            parts[0]
        )),
    }
}

fn help_text() -> String {
    "\
Token Management
================

Securely store API keys and tokens for MCP servers.

Commands:
  /token                         List all configured tokens (masked)
  /token set <server> <KEY> <val> Set a token for an MCP server
  /token set <KEY> <val>         Set token (auto-detects server if only one)
  /token remove <server> <KEY>   Remove a token
  /token help                    This message

Security:
  - Tokens are stored in your config file, NOT in chat history
  - Messages containing tokens are auto-deleted from Telegram
  - Tokens are masked when displayed (only first/last 4 chars shown)

Examples:
  /token set github GITHUB_PERSONAL_ACCESS_TOKEN ghp_xxxxxxxxxxxx
  /token set linear LINEAR_API_TOKEN lin_xxxxxxxxxxxx
  /token remove github GITHUB_PERSONAL_ACCESS_TOKEN"
        .into()
}

fn list_tokens() -> Result<String> {
    let path = config_path();
    let config = load_config(&path)?;

    let mut lines = vec!["Configured Tokens".to_string(), String::new()];

    if config.mcp.servers.is_empty() {
        lines.push("No MCP servers configured.".into());
        lines.push("Add one first: /mcp add <name> <command>".into());
        return Ok(lines.join("\n"));
    }

    let mut has_any = false;
    for server in &config.mcp.servers {
        if server.env.is_empty() {
            continue;
        }
        has_any = true;
        lines.push(format!("{}:", server.name));
        for (key, value) in &server.env {
            lines.push(format!("  {} = {}", key, mask_secret(value)));
        }
    }

    if !has_any {
        lines.push("No tokens configured.".into());
        lines.push("Set one: /token set <server> <KEY> <value>".into());
    }

    Ok(lines.join("\n"))
}

fn set_server_token(path: &PathBuf, server_name: &str, key: &str, value: &str) -> Result<String> {
    let mut config = load_config(path)?;

    let server = config
        .mcp
        .servers
        .iter_mut()
        .find(|s| s.name == server_name)
        .ok_or_else(|| anyhow!("No MCP server named '{}'", server_name))?;

    let is_update = server.env.contains_key(key);
    server.env.insert(key.to_string(), value.to_string());
    save_config(path, &config)?;

    let action = if is_update { "Updated" } else { "Set" };
    Ok(format!(
        "{} {} = {} for server '{}'.\nUse /mcp enable {} to activate if not already enabled.",
        action, key, mask_secret(value), server_name, server_name
    ))
}

fn remove_server_token(path: &PathBuf, server_name: &str, key: &str) -> Result<String> {
    let mut config = load_config(path)?;

    let server = config
        .mcp
        .servers
        .iter_mut()
        .find(|s| s.name == server_name)
        .ok_or_else(|| anyhow!("No MCP server named '{}'", server_name))?;

    if server.env.remove(key).is_some() {
        save_config(path, &config)?;
        Ok(format!("Removed {} from server '{}'.", key, server_name))
    } else {
        Err(anyhow!("Key '{}' not found on server '{}'", key, server_name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_secret() {
        assert_eq!(mask_secret(""), "(not set)");
        assert_eq!(mask_secret("short"), "*****");
        assert_eq!(mask_secret("ghp_1234567890abcdef"), "ghp_...cdef");
    }

    #[test]
    fn test_contains_secret() {
        assert!(contains_secret("my token is ghp_abc123def456"));
        assert!(contains_secret("sk-live-1234567890"));
        assert!(contains_secret("use Bearer eyJhbGciOiJIUzI1NiJ9.payload.signature"));
        assert!(contains_secret("GITHUB_PERSONAL_ACCESS_TOKEN=gho_abc123"));
        assert!(!contains_secret("hello world"));
        assert!(!contains_secret("just a normal message"));
    }

    #[test]
    fn test_help() {
        let resp = help_text();
        assert!(resp.contains("Token Management"));
        assert!(resp.contains("/token set"));
    }
}
