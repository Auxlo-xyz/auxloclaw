//! /model command - Per-user model and provider override
//!
//! Usage:
//!   /model                     - Show current override
//!   /model <model_id>          - Set model ID only
//!   /model base <url>          - Set base URL only
//!   /model key <api_key>       - Set API key only (encrypted at rest)
//!   /model base <url> key <k>  - Set base URL + key
//!   /model <model_id> base <url> key <k> - Set all three
//!   /model reset               - Clear override, revert to defaults

use crate::memory::model_store::{ModelStore, UserModelOverride};
use anyhow::Result;
use std::sync::Arc;

/// Parse and handle the /model command.
///
/// `channel` is "telegram" or "discord"
/// `user_id` is the platform-specific user identifier
/// `args` is everything after "/model" trimmed
pub fn handle_model(
    store: &ModelStore,
    channel: &str,
    user_id: &str,
    args: &str,
) -> Result<String> {
    let args = args.trim();

    // /model with no args - show current
    if args.is_empty() {
        return show_current(store, channel, user_id);
    }

    // /model reset
    if args == "reset" {
        if store.delete(channel, user_id)? {
            return Ok("Model override cleared. Using global defaults.".into());
        } else {
            return Ok("No model override was set.".into());
        }
    }

    // Parse arguments
    let mut model_id: Option<String> = None;
    let mut base_url: Option<String> = None;
    let mut api_key: Option<String> = None;

    let tokens: Vec<&str> = args.split_whitespace().collect();
    let mut i = 0;
    while i < tokens.len() {
        match tokens[i] {
            "base" if i + 1 < tokens.len() => {
                base_url = Some(tokens[i + 1].to_string());
                i += 2;
            }
            "key" if i + 1 < tokens.len() => {
                api_key = Some(tokens[i + 1].to_string());
                i += 2;
            }
            other => {
                // Treat as model_id if it doesn't look like a flag value
                if model_id.is_none() && !other.starts_with("http") {
                    model_id = Some(other.to_string());
                }
                i += 1;
            }
        }
    }

    // Load existing or create new
    let mut ov = store.get(channel, user_id)?.unwrap_or_default();

    let mut changes = Vec::new();

    if let Some(m) = model_id {
        ov.model_id = Some(m.clone());
        changes.push(format!("Model: {}", m));
    }
    if let Some(b) = base_url {
        ov.base_url = Some(b.clone());
        changes.push(format!("Base URL: {}", b));
    }
    if let Some(k) = api_key {
        let encrypted = store.encrypt_key(&k)?;
        ov.encrypted_api_key = Some(encrypted);
        let masked = mask_key(&k);
        changes.push(format!("API Key: {}", masked));
    }

    if changes.is_empty() {
        return show_current(store, channel, user_id);
    }

    ov.updated_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    store.set(channel, user_id, &ov)?;

    let mut response = format!("Model settings updated:\n{}", changes.join("\n"));

    if let Some(ref m) = ov.model_id {
        response.push_str(&format!("\nActive model: {}", m));
    }

    Ok(response)
}

fn show_current(store: &ModelStore, channel: &str, user_id: &str) -> Result<String> {
    match store.get(channel, user_id)? {
        Some(ov) => {
            let mut lines = vec!["Current model override:".to_string()];
            lines.push(format!(
                "  Model: {}",
                ov.model_id.as_deref().unwrap_or("(not set)")
            ));
            lines.push(format!(
                "  Base URL: {}",
                ov.base_url.as_deref().unwrap_or("(not set)")
            ));
            lines.push(format!(
                "  API Key: {}",
                if ov.encrypted_api_key.is_some() {
                    "(set, encrypted)"
                } else {
                    "(not set)"
                }
            ));
            lines.push("\nUsage: /model <model_id> base <url> key <api_key>".into());
            lines.push("Reset: /model reset".into());
            Ok(lines.join("\n"))
        }
        None => Ok(
            "No model override set. Using global defaults.\n\n\
             Usage: /model <model_id> base <url> key <api_key>\n\
             Example: /model gpt-4o base https://api.openai.com/v1 key sk-xxx\n\
             Reset: /model reset"
                .into(),
        ),
    }
}

/// Mask an API key for display: show first 4 and last 4 chars.
fn mask_key(key: &str) -> String {
    if key.len() <= 12 {
        format!("{}***", &key[..4.min(key.len())])
    } else {
        format!("{}***{}", &key[..4], &key[key.len() - 4..])
    }
}

/// Resolve the effective model config for a user session.
/// Returns (base_url, api_key, model_id) where each field falls back to None
/// if the user hasn't overridden it.
pub fn resolve_user_model(
    store: &ModelStore,
    channel: &str,
    user_id: &str,
) -> Result<(Option<String>, Option<String>, Option<String>)> {
    match store.get(channel, user_id)? {
        Some(ov) => {
            let api_key = match &ov.encrypted_api_key {
                Some(enc) => Some(store.decrypt_key(enc)?),
                None => None,
            };
            Ok((ov.base_url, api_key, ov.model_id))
        }
        None => Ok((None, None, None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_store() -> ModelStore {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("auxloclaw_cmd_test_{}", ts));
        fs::create_dir_all(&dir).unwrap();
        ModelStore::new(&dir).unwrap()
    }

    #[test]
    fn test_show_no_override() {
        let store = temp_store();
        let resp = handle_model(&store, "telegram", "999", "").unwrap();
        assert!(resp.contains("No model override"));
    }

    #[test]
    fn test_set_model_id() {
        let store = temp_store();
        let resp = handle_model(&store, "telegram", "1", "gpt-4o").unwrap();
        assert!(resp.contains("Model: gpt-4o"));
    }

    #[test]
    fn test_set_base_and_key() {
        let store = temp_store();
        let resp = handle_model(
            &store,
            "telegram",
            "2",
            "gpt-4o base https://api.openai.com/v1 key sk-test12345678",
        )
        .unwrap();
        assert!(resp.contains("Model: gpt-4o"));
        assert!(resp.contains("Base URL: https://api.openai.com/v1"));
        assert!(resp.contains("API Key: sk-t***5678"));
    }

    #[test]
    fn test_reset() {
        let store = temp_store();
        handle_model(&store, "telegram", "3", "gpt-4o").unwrap();
        let resp = handle_model(&store, "telegram", "3", "reset").unwrap();
        assert!(resp.contains("cleared"));
        assert!(store.get("telegram", "3").unwrap().is_none());
    }

    #[test]
    fn test_resolve_user_model() {
        let store = temp_store();
        handle_model(
            &store,
            "telegram",
            "4",
            "claude-3 base https://api.anthropic.com key sk-ant-secret123",
        )
        .unwrap();

        let (base, key, model) = resolve_user_model(&store, "telegram", "4").unwrap();
        assert_eq!(base.as_deref(), Some("https://api.anthropic.com"));
        assert_eq!(key.as_deref(), Some("sk-ant-secret123"));
        assert_eq!(model.as_deref(), Some("claude-3"));
    }

    #[test]
    fn test_mask_key() {
        assert_eq!(mask_key("sk-1234567890"), "sk-1***7890");
        assert_eq!(mask_key("short"), "shor***");
    }
}
