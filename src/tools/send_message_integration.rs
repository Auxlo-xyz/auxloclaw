//! Message router integration example.
//!
//! This file shows how to wire the MessageRouter into auxloclaw's
//! gateway startup in main.rs. The pattern:
//!
//! 1. Create MessageRouter before Telegram/Discord tasks spawn
//! 2. Register SendMessageTool with the orchestrator (router is shared)
//! 3. Pass router to channel adapters
//! 4. Channel adapters register themselves after creating their bot client
//!
//! This way the tool is always available, and gracefully reports
//! "not connected" if the platform hasn't started yet.

// === Add to main.rs after orchestrator creation (~line 221) ===
//
// // Create shared message router for cross-platform messaging
// let mut message_router = tools::MessageRouter::new();
// message_router.set_default_platform("telegram".to_string());
// let message_router = Arc::new(message_router);
//
// // Register send_message tool with the orchestrator
// raw_orchestrator.register_send_message_tool(message_router.clone());
//
// === Pass router to channel starts ===
//
// Change Telegram start from:
//   channels::telegram::start(tg_agent, model_store, code_mode, Some(tg_config), tg_persona)
// To:
//   channels::telegram::start(tg_agent, model_store, code_mode, Some(tg_config), tg_persona, Some(message_router.clone()))
//
// Change Discord start from:
//   channels::discord::start(discord_agent, model_store_discord, code_mode_discord, Some(discord_config))
// To:
//   channels::discord::start(discord_agent, model_store_discord, code_mode_discord, Some(discord_config), Some(message_router.clone()))
//
// === Inside channels/telegram.rs start() function ===
//
// After the bot is created, register with the router:
//
// if let Some(router) = message_router {
//     let adapter = Arc::new(tools::send_message::TelegramAdapter::new(
//         bot.clone(),
//         Some(config.default_chat_id.unwrap_or(0)),
//     ));
//     router.register(adapter).await;
//     tracing::info!("Telegram registered with message router");
// }
//
// === Add to orchestrator/mod.rs ===
//
// impl ToolOrchestrator {
//     pub fn register_send_message_tool(&mut self, router: Arc<tools::MessageRouter>) {
//         let tool = Arc::new(tools::SendMessageTool::new((*router).clone()));
//         self.register(tool);
//     }
// }
