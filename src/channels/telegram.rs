//! Telegram channel adapter with full command support

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use teloxide::{
    dispatching::Dispatcher,
    prelude::*,
    types::{ChatAction, ParseMode, Update, ChatId},
    utils::command::BotCommands,
    Bot,
};

use crate::agent::AgentCore;
use crate::config::TelegramConfig;
use crate::persona::PersonaConfig;

/// Telegram commands
#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase")]
pub enum Command {
    #[command(description = "View agent memory")]
    Memory,

    #[command(description = "Clear conversation history")]
    Clear,

    #[command(description = "List available tools")]
    Tools,

    #[command(description = "View token usage statistics")]
    Usage,

    #[command(description = "Recover from crashed session")]
    Recover,

    #[command(description = "Show help message")]
    Help,

    #[command(description = "Check bot status")]
    Status,

    #[command(description = "Toggle voice mode or set voice")]
    Voice,

    #[command(description = "Manage agent personas")]
    Persona,

    #[command(description = "Start a new session")]
    New,
}

/// Session state per chat
#[derive(Debug, Clone, Default)]
struct SessionState {
    message_count: u64,
    total_tokens: u64,
    prompt_tokens: u64,
    completion_tokens: u64,
    created_at: chrono::DateTime<chrono::Utc>,
    voice_mode: bool,
    voice_id: Option<String>,
    last_activity: chrono::DateTime<chrono::Utc>,
}

/// Global Telegram state
pub struct TelegramState {
    agent: Arc<AgentCore>,
    sessions: RwLock<HashMap<i64, SessionState>>,
    config: TelegramConfig,
    persona: PersonaConfig,
}

impl TelegramState {
    pub fn new(agent: Arc<AgentCore>, config: TelegramConfig, persona: PersonaConfig) -> Self {
        Self {
            agent,
            sessions: RwLock::new(HashMap::new()),
            config,
            persona,
        }
    }

    async fn get_or_create_session(&self, chat_id: i64) -> SessionState {
        let mut sessions = self.sessions.write().await;
        sessions
            .entry(chat_id)
            .or_insert_with(|| SessionState {
                created_at: chrono::Utc::now(),
                last_activity: chrono::Utc::now(),
                ..Default::default()
            })
            .clone()
    }

    async fn update_session(&self, chat_id: i64, tokens: Option<(u32, u32)>) {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(&chat_id) {
            session.message_count += 1;
            session.last_activity = chrono::Utc::now();
            if let Some((prompt, completion)) = tokens {
                session.prompt_tokens += prompt as u64;
                session.completion_tokens += completion as u64;
                session.total_tokens += (prompt + completion) as u64;
            }
        }
    }

    async fn clear_session(&self, chat_id: i64) {
        let mut sessions = self.sessions.write().await;
        sessions.insert(
            chat_id,
            SessionState {
                created_at: chrono::Utc::now(),
                last_activity: chrono::Utc::now(),
                ..Default::default()
            },
        );
    }
}

/// Start Telegram gateway
pub async fn start(agent: Arc<AgentCore>, config: Option<TelegramConfig>, persona: PersonaConfig) -> Result<()> {
    let config = config.ok_or_else(|| anyhow!("Telegram config required"))?;

    if !config.enabled || config.token.is_empty() {
        tracing::info!("📱 Telegram gateway disabled");
        return Ok(());
    }

    tracing::info!(
        "📱 Telegram gateway started (token: {}...)",
        &config.token.chars().take(8).collect::<String>()
    );

    let bot = Bot::new(config.token.clone());
    let state = Arc::new(TelegramState::new(agent, config, persona));

    // Set bot commands
    let commands = vec![
        teloxide::types::BotCommand { command: "memory".into(), description: "View agent memory".into() },
        teloxide::types::BotCommand { command: "clear".into(), description: "Clear conversation history".into() },
        teloxide::types::BotCommand { command: "tools".into(), description: "List available tools".into() },
        teloxide::types::BotCommand { command: "usage".into(), description: "View token usage statistics".into() },
        teloxide::types::BotCommand { command: "recover".into(), description: "Recover from crashed session".into() },
        teloxide::types::BotCommand { command: "help".into(), description: "Show help message".into() },
        teloxide::types::BotCommand { command: "status".into(), description: "Check bot status".into() },
        teloxide::types::BotCommand { command: "voice".into(), description: "Toggle voice mode or set voice".into() },
        teloxide::types::BotCommand { command: "persona".into(), description: "Manage agent personas".into() },
        teloxide::types::BotCommand { command: "new".into(), description: "Start a new session".into() },
    ];
    let _ = bot.set_my_commands(commands).await;

    // Create handler
    let handler = dptree::entry()
        .branch(Update::filter_message().filter_command::<Command>().endpoint(handle_command))
        .branch(Update::filter_message().endpoint(handle_message));

    // Start dispatcher
    tokio::spawn(async move {
        Dispatcher::builder(bot, handler)
            .dependencies(dptree::deps![state])
            .build()
            .dispatch()
            .await;
    });

    // Wait forever
    tokio::signal::ctrl_c().await?;
    Ok(())
}

/// Handle slash commands
async fn handle_command(
    bot: Bot,
    msg: Message,
    cmd: Command,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    let chat_id: i64 = msg.chat.id.0;

    // Show typing indicator for commands too
    let _ = bot.send_chat_action(ChatId(chat_id), ChatAction::Typing).await;

    let response = match cmd {
        Command::Memory => {
            let memory = state.agent.memory_summary().await;
            format!(
                "🧠 *Agent Memory*\n\n{}\n\n_Use /clear to reset session_",
                escape_md(&memory)
            )
        }

        Command::Clear => {
            state.clear_session(chat_id).await;
            let _ = state.agent.clear_session(&format!("tg:{}", chat_id)).await;
            "🗑️ *Session Cleared*\n\nYour conversation history has been reset\\. Starting fresh!".to_string()
        }

        Command::Tools => {
            let tools = state.agent.list_tools();
            let mut list = String::from("🔧 *Available Tools*\n\n");
            for tool in tools {
                list.push_str(&format!("• *{}* \\- {}\n", escape_md(&tool.name), escape_md(&tool.description)));
            }
            list.push_str("\n_Use a tool by describing what you need in your message_");
            list
        }

        Command::Usage => {
            let session = state.get_or_create_session(chat_id).await;
            let usage = state.agent.get_usage_stats().await;
            format!(
                "📊 *Token Usage Statistics*\n\n\
                *Current Session:*\n\
                • Messages: {}\n\
                • Total Tokens: {}\n\
                • Prompt Tokens: {}\n\
                • Completion Tokens: {}\n\n\
                *All\\-Time:*\n\
                • Total Messages: {}\n\
                • Total Tokens: {}\n\n\
                _Session started: {}_",
                session.message_count,
                session.total_tokens,
                session.prompt_tokens,
                session.completion_tokens,
                usage.total_messages,
                usage.total_tokens,
                session.created_at.format("%Y\\-%m\\-%d %H:%M UTC")
            )
        }

        Command::Recover => {
            let _ = state.agent.recover_session(&format!("tg:{}", chat_id)).await;
            "♻️ *Session Recovered*\n\nYour session has been restored from the last known state\\.".to_string()
        }

        Command::Help => {
            format!(
                "🤖 *{} Help*\n\n\
                I'm an AI assistant\\. Send me any message and I'll help you\\!\n\n\
                *Commands:*\n\
                /memory \\- View agent memory\n\
                /clear \\- Clear conversation history\n\
                /tools \\- List available tools\n\
                /usage \\- View token usage\n\
                /recover \\- Recover from crash\n\
                /status \\- Check bot status\n\
                /voice \\- Toggle voice mode\n\
                /persona \\- Manage personas\n\
                /new \\- Start new session\n\n\
                _Tips:_\n\
                • Be specific in your requests\n\
                • I can use tools to help you\n\
                • Use /new to reset context",
                escape_md(&state.persona.name)
            )
        }

        Command::Status => {
            let session = state.get_or_create_session(chat_id).await;
            let uptime = chrono::Utc::now() - session.created_at;
            format!(
                "✅ *Bot Status*\n\n\
                • Status: Online\n\
                • Uptime: {}h {}m\n\
                • Active chats: {}\n\
                • Model: {}\n\
                • Voice mode: {}\n\
                • Last activity: {}",
                uptime.num_hours(),
                uptime.num_minutes() % 60,
                state.sessions.read().await.len(),
                escape_md(state.agent.model_name()),
                if session.voice_mode { "ON" } else { "OFF" },
                session.last_activity.format("%H:%M:%S UTC")
            )
        }

        Command::Voice => {
            let mut sessions = state.sessions.write().await;
            if let Some(session) = sessions.get_mut(&chat_id) {
                session.voice_mode = !session.voice_mode;
                let status = if session.voice_mode { "ON" } else { "OFF" };
                format!("🎤 *Voice Mode: {}*\n\n{}", status, 
                    if session.voice_mode {
                        "I'll respond with voice messages\\. Use /voice again to toggle off\\."
                    } else {
                        "Voice mode disabled\\. I'll respond with text\\."
                    }
                )
            } else {
                "❌ No active session\\. Send a message first\\!".to_string()
            }
        }

        Command::Persona => {
            format!(
                "🎭 *Current Persona*\n\n\
                *Name:* {}\n\n\
                *Behavior:*\n{}\n\n\
                *Style:*\n\
                • Length: {}\n\
                • Tone: {}\n\
                • No em dashes: {}\n\
                • No emojis: {}\n\n\
                _Edit persona via CLI: auxloclaw persona_",
                escape_md(&state.persona.name),
                escape_md(&state.persona.behavior),
                state.persona.style.length,
                state.persona.style.tone,
                state.persona.style.formatting.no_em_dashes,
                state.persona.style.formatting.no_emojis
            )
        }

        Command::New => {
            state.clear_session(chat_id).await;
            let _ = state.agent.new_session(&format!("tg:{}", chat_id)).await;
            "🆕 *New Session Started*\n\nPrevious context cleared\\. I'm ready for a fresh conversation\\!\n\n_What would you like to discuss?_".to_string()
        }
    };

    bot.send_message(ChatId(chat_id), response)
        .parse_mode(ParseMode::MarkdownV2)
        .await?;

    Ok(())
}

/// Handle regular messages
async fn handle_message(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    let chat_id: i64 = msg.chat.id.0;
    let text = msg.text().unwrap_or("");

    if text.is_empty() {
        return Ok(());
    }

    // Show typing indicator while processing
    bot.send_chat_action(ChatId(chat_id), ChatAction::Typing).await?;

    // Get session
    let session = state.get_or_create_session(chat_id).await;

    // Process with agent
    let response = state.agent.process(text).await;

    // Update session stats
    state.update_session(chat_id, None).await;

    // Check voice mode
    if session.voice_mode {
        bot.send_message(ChatId(chat_id), format!("🎤 {}", escape_md(&response)))
            .parse_mode(ParseMode::MarkdownV2)
            .await?;
    } else {
        bot.send_message(ChatId(chat_id), escape_md(&response))
            .parse_mode(ParseMode::MarkdownV2)
            .await?;
    }

    Ok(())
}

/// Escape text for MarkdownV2
fn escape_md(text: &str) -> String {
    text.replace('_', "\\_")
        .replace('*', "\\*")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace('(', "\\(")
        .replace(')', "\\)")
        .replace('~', "\\~")
        .replace('`', "\\`")
        .replace('>', "\\>")
        .replace('#', "\\#")
        .replace('+', "\\+")
        .replace('-', "\\-")
        .replace('=', "\\=")
        .replace('|', "\\|")
        .replace('{', "\\{")
        .replace('}', "\\}")
        .replace('.', "\\.")
        .replace('!', "\\!")
}
