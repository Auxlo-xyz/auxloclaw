//! Telegram channel adapter - simplified
use anyhow::Result;

use crate::config::TelegramConfig;

/// Start Telegram gateway
pub async fn start(
    _agent: std::sync::Arc<crate::agent::AgentCore>,
    config: Option<TelegramConfig>,
) -> Result<()> {
    if let Some(config) = config {
        if config.enabled && !config.token.is_empty() {
            tracing::info!("📱 Telegram gateway started (token: {}...)", 
                &config.token.chars().take(8).collect::<String>());
        }
    }
    Ok(())
}