//! Discord channel adapter
use anyhow::Result;
use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::prelude::*;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};

pub struct DiscordHandler {
    agent: Arc<crate::agent::AgentCore>,
    model_store: Arc<crate::memory::model_store::ModelStore>,
    coding_users: Arc<RwLock<HashSet<u64>>>,
}

impl DiscordHandler {
    pub fn new(agent: Arc<crate::agent::AgentCore>, model_store: Arc<crate::memory::model_store::ModelStore>) -> Self {
        Self {
            agent,
            model_store,
            coding_users: Arc::new(RwLock::new(HashSet::new())),
        }
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

            let content_clone = content.clone();
            let coding_users = self.coding_users.clone();
            tokio::spawn(async move {
                // Check for /code command
                if content_clone.trim().starts_with("/code") {
                    let session_id = format!("discord-code-{}", user_id);
                    let workspace = crate::commands::code::ensure_workspace(&session_id)
                        .unwrap_or_else(|e| {
                            tracing::warn!("Failed to create workspace: {}", e);
                            std::path::PathBuf::from("/tmp/auxloclaw-code")
                        });
                    let _ = crate::commands::code::init_workspace(&workspace);
                    let code_prompt = crate::commands::code::build_code_system_prompt(&workspace);
                    agent.set_system_prompt_override(&format!("dc:{}", msg.author.id.get()), code_prompt).await;
                    coding_users.write().await.insert(user_id.get());
                    let response = format!(
                        "Coding mode activated.\nWorkspace: {}\n\nSend your coding task as the next message. Use /normal to exit coding mode.",
                        workspace.display()
                    );
                    if let Err(e) = msg_channel.say(&http, response).await {
                        error!("Failed to send Discord message: {}", e);
                    }
                    return;
                }

                // Check for /normal to exit code mode
                if content_clone.trim() == "/normal" {
                    coding_users.write().await.remove(&user_id.get());
                    agent.clear_system_prompt_override(&format!("dc:{}", msg.author.id.get())).await;
                    if let Err(e) = msg_channel.say(&http, "Exited coding mode. Back to normal.").await {
                        error!("Failed to send Discord message: {}", e);
                    }
                    return;
                }

                // Check for /update command before passing to agent
                if content_clone.trim().starts_with("/update") {
                    let result = crate::commands::update::handle_update().await;
                    if let Err(e) = msg_channel.say(&http, result).await {
                        error!("Failed to send Discord message: {}", e);
                    }
                    return;
                }

                // Route through code session if in code mode
                let is_coding = coding_users.read().await.contains(&user_id.get());
                let session_id = if is_coding {
                    Some(format!("discord-code-{}", user_id))
                } else {
                    Some(user_id.to_string())
                };
                let _typing = msg_channel.start_typing(&http);
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
    model_store: Arc<crate::memory::model_store::ModelStore>,
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
                .event_handler(DiscordHandler::new(agent, model_store))
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
