//! Discord channel adapter
use anyhow::Result;
use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::prelude::*;
use std::sync::Arc;
use tracing::{error, info};

pub struct DiscordHandler {
    agent: Arc<crate::agent::AgentCore>,
}

impl DiscordHandler {
    pub fn new(agent: Arc<crate::agent::AgentCore>) -> Self {
        Self { agent }
    }
}

#[async_trait]
impl EventHandler for DiscordHandler {
    async fn ready(&self, _: Context, ready: Ready) {
        info!("🚀 Discord gateway connected as {}", ready.user.name);
    }

    async fn message(&self, ctx: Context, msg: Message) {
        // Ignore messages from the bot itself
        if msg.author.bot {
            return;
        }

        let bot_user = ctx.cache.current_user();
        let is_dm = msg.guild_id.is_none();
        let is_mention = msg.mentions.iter().any(|u| u.id == bot_user.id);

        if is_dm || is_mention {
            let user_id = msg.author.id;
            let content = msg
                .content
                .replace(&format!("<@{}>", bot_user.id), "")
                .replace(&format!("<@!{}>", bot_user.id), "")
                .trim()
                .to_string();

            info!("📩 Message from {}: {}", user_id, content);

            // We spawn a task to handle the agent processing so we don't block the gateway
            let agent = Arc::clone(&self.agent);
            let msg_channel = msg.channel_id;
            let http = ctx.http;

            tokio::spawn(async move {
                // Use the agent to process the message
                let session_id = Some(user_id.to_string());
                let response = agent.process(&content, session_id.as_deref()).await;

                // Use the simple say method for serenity 0.12
                if let Err(e) = msg_channel.say(&http, response).await {
                    error!("Failed to send Discord message: {}", e);
                }
            });
        }
    }
}

/// Start Discord gateway
pub async fn start(
    agent: Arc<crate::agent::AgentCore>,
    config: Option<crate::config::DiscordConfig>,
) -> Result<()> {
    if let Some(config) = config {
        if config.enabled && !config.token.is_empty() {
            info!("💬 Initializing Discord gateway...");

            let intents = GatewayIntents::GUILD_MESSAGES
                | GatewayIntents::DIRECT_MESSAGES
                | GatewayIntents::MESSAGE_CONTENT
                | GatewayIntents::GUILD_MEMBERS;

            let mut client = Client::builder(&config.token, intents)
                .event_handler(DiscordHandler::new(agent))
                .await
                .map_err(|e| anyhow::anyhow!("Discord client builder error: {}", e))?;

            // Spawn the client in a separate task so it doesn't block the main thread
            tokio::spawn(async move {
                if let Err(e) = client.start().await {
                    error!("❌ Discord client error: {}", e);
                }
            });

            info!("✅ Discord gateway started and running in background");
        } else {
            info!("💤 Discord gateway is disabled in config");
        }
    }
    Ok(())
}
