//! Discord channel adapter - simplified
use anyhow::Result;

use crate::config::DiscordConfig;

/// Start Discord gateway
pub async fn start(
    _agent: std::sync::Arc<crate::agent::AgentCore>,
    config: Option<DiscordConfig>,
) -> Result<()> {
    if let Some(config) = config {
        if config.enabled && !config.token.is_empty() {
            tracing::info!("💬 Discord gateway started");
        }
    }
    Ok(())
}