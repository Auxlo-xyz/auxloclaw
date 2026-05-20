//! Telegram channel adapter with full command support.

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use teloxide::{
    dispatching::Dispatcher,
    prelude::*,
    types::{ChatAction, ChatId, Update},
    utils::command::BotCommands,
    Bot,
};

use crate::agent::AgentCore;
use crate::channels::markdown::markdown_to_telegram;
use crate::config::TelegramConfig;
use crate::persona::shared::{
    load_current_persona, reset_persona, set_behavior, set_length, set_name, set_no_em_dashes,
    set_no_emojis, set_tone, toggle_no_em_dashes, toggle_no_emojis,
};

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
    #[command(description = "Manage agent persona")]
    Persona,
    #[command(description = "Update auxloclaw to the latest version")]
    Update,
    #[command(description = "Start a coding session in isolated workspace")]
    Code,
    #[command(description = "Start a new session")]
    New,
}

#[derive(Debug, Clone)]
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

impl Default for SessionState {
    fn default() -> Self {
        let now = chrono::Utc::now();
        Self {
            message_count: 0,
            total_tokens: 0,
            prompt_tokens: 0,
            completion_tokens: 0,
            created_at: now,
            voice_mode: false,
            voice_id: None,
            last_activity: now,
        }
    }
}

pub struct TelegramState {
    agent: Arc<AgentCore>,
    sessions: RwLock<HashMap<i64, SessionState>>,
    config: TelegramConfig,
}

impl TelegramState {
    pub fn new(agent: Arc<AgentCore>, config: TelegramConfig) -> Self {
        Self {
            agent,
            sessions: RwLock::new(HashMap::new()),
            config,
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

pub async fn start(
    agent: Arc<AgentCore>,
    config: Option<TelegramConfig>,
    _persona: crate::persona::PersonaConfig,
) -> Result<()> {
    let config = config.ok_or_else(|| anyhow!("Telegram config required"))?;
    if !config.enabled || config.token.is_empty() {
        tracing::info!("Telegram gateway disabled");
        return Ok(());
    }

    let bot = Bot::new(config.token.clone());
    let state = Arc::new(TelegramState::new(agent, config));

    let commands = vec![
        teloxide::types::BotCommand {
            command: "memory".into(),
            description: "View agent memory".into(),
        },
        teloxide::types::BotCommand {
            command: "clear".into(),
            description: "Clear conversation history".into(),
        },
        teloxide::types::BotCommand {
            command: "tools".into(),
            description: "List available tools".into(),
        },
        teloxide::types::BotCommand {
            command: "usage".into(),
            description: "View token usage statistics".into(),
        },
        teloxide::types::BotCommand {
            command: "recover".into(),
            description: "Recover from crashed session".into(),
        },
        teloxide::types::BotCommand {
            command: "help".into(),
            description: "Show help message".into(),
        },
        teloxide::types::BotCommand {
            command: "status".into(),
            description: "Check bot status".into(),
        },
        teloxide::types::BotCommand {
            command: "voice".into(),
            description: "Toggle voice mode or set voice".into(),
        },
        teloxide::types::BotCommand {
            command: "persona".into(),
            description: "Show or edit persona".into(),
        },
        teloxide::types::BotCommand {
            command: "new".into(),
            description: "Start new session".into(),
        },
        teloxide::types::BotCommand {
            command: "update".into(),
            description: "Update auxloclaw to the latest version".into(),
        },
        teloxide::types::BotCommand {
            command: "code".into(),
            description: "Start a coding session in isolated workspace".into(),
        },
    ];
    let _ = bot.set_my_commands(commands).await;

    let handler = dptree::entry()
        .branch(
            Update::filter_message()
                .filter_command::<Command>()
                .endpoint(handle_command),
        )
        .branch(Update::filter_message().endpoint(handle_message));

    tokio::spawn(async move {
        Dispatcher::builder(bot, handler)
            .dependencies(dptree::deps![state])
            .build()
            .dispatch()
            .await;
    });

    tokio::signal::ctrl_c().await?;
    Ok(())
}

fn render_persona() -> String {
    match load_current_persona() {
        Ok(persona) => format!(
            "Current persona\n\nName: {}\nLength: {}\nTone: {}\nNo em dashes: {}\nNo emojis: {}\n\nBehavior:\n{}\n\nCommands:\n/persona show\n/persona name <value>\n/persona behavior <value>\n/persona tone <professional|casual|technical|friendly>\n/persona length <concise|balanced|detailed>\n/persona no_emojis <on|off>\n/persona no_em_dashes <on|off>\n/persona reset",
            persona.name,
            persona.style.length,
            persona.style.tone,
            persona.style.formatting.no_em_dashes,
            persona.style.formatting.no_emojis,
            persona.behavior
        ),
        Err(e) => format!("Failed to load persona: {e}"),
    }
}

fn handle_persona_command_text(text: &str) -> String {
    let trimmed = text.trim();
    let rest = trimmed.strip_prefix("/persona").unwrap_or("").trim();
    if rest.is_empty() || rest == "show" {
        return render_persona();
    }

    let mut parts = rest.splitn(2, ' ');
    let sub = parts.next().unwrap_or("").trim();
    let arg = parts.next().unwrap_or("").trim();

    let result = match sub {
        "name" if !arg.is_empty() => set_name(arg).map(|_| "Persona name updated".to_string()),
        "behavior" if !arg.is_empty() => set_behavior(arg).map(|_| "Persona behavior updated".to_string()),
        "tone" if !arg.is_empty() => set_tone(arg).map(|_| format!("Persona tone set to {arg}")),
        "length" if !arg.is_empty() => set_length(arg).map(|_| format!("Persona length set to {arg}")),
        "no_emojis" if !arg.is_empty() => {
            let enabled = matches!(arg, "on" | "true" | "yes" | "1");
            set_no_emojis(enabled).map(|_| format!("Persona no_emojis set to {enabled}"))
        }
        "no_emojis" => toggle_no_emojis().map(|enabled| format!("Persona no_emojis set to {enabled}")),
        "no_em_dashes" if !arg.is_empty() => {
            let enabled = matches!(arg, "on" | "true" | "yes" | "1");
            set_no_em_dashes(enabled).map(|_| format!("Persona no_em_dashes set to {enabled}"))
        }
        "no_em_dashes" => toggle_no_em_dashes().map(|enabled| format!("Persona no_em_dashes set to {enabled}")),
        "reset" => reset_persona().map(|_| "Persona reset to defaults".to_string()),
        _ => Err(anyhow!("Invalid persona command. Use /persona show, name, behavior, tone, length, no_emojis, no_em_dashes, or reset")),
    };

    match result {
        Ok(msg) => format!("{}\n\n{}", msg, render_persona()),
        Err(e) => format!("{}\n\n{}", e, render_persona()),
    }
}

async fn handle_command(
    bot: Bot,
    msg: Message,
    cmd: Command,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    let chat_id: i64 = msg.chat.id.0;
    let _ = bot
        .send_chat_action(ChatId(chat_id), ChatAction::Typing)
        .await;

    let response = match cmd {
        Command::Memory => state.agent.memory_summary().await,
        Command::Clear => {
            let session_id = format!("tg:{}", chat_id);
            state.agent.clear_session(&session_id).await;
            state.sessions.write().await.remove(&chat_id);
            "Session cleared".to_string()
        }
        Command::Tools => {
            let tools = state.agent.list_tools();
            let mut list = String::from("Available tools\n\n");
            for tool in tools {
                list.push_str(&format!("- {}: {}\n", tool.name, tool.description));
            }
            list
        }
        Command::Usage => {
            let session = state.get_or_create_session(chat_id).await;
            let usage = state.agent.get_usage_stats().await;
            format!(
                "Current session\nMessages: {}\nTotal tokens: {}\nPrompt tokens: {}\nCompletion tokens: {}\n\nAll-time\nMessages: {}\nTokens: {}",
                session.message_count,
                session.total_tokens,
                session.prompt_tokens,
                session.completion_tokens,
                usage.total_messages,
                usage.total_tokens,
            )
        }
        Command::Recover => {
            let _ = state
                .agent
                .recover_session(&format!("tg:{}", chat_id))
                .await;
            "Session recovered".to_string()
        }
        Command::Help => {
            "Commands: /memory /clear /tools /usage /recover /status /voice /persona /new /update /code"
                .to_string()
        }
        Command::Status => {
            let session = state.get_or_create_session(chat_id).await;
            let uptime = chrono::Utc::now() - session.created_at;
            format!(
                "Status: Online\nUptime: {}h {}m\nActive chats: {}\nModel: {}\nVoice mode: {}",
                uptime.num_hours(),
                uptime.num_minutes() % 60,
                state.sessions.read().await.len(),
                state.agent.model_name(),
                if session.voice_mode { "ON" } else { "OFF" },
            )
        }
        Command::Voice => {
            let mut sessions = state.sessions.write().await;
            let session = sessions.entry(chat_id).or_default();
            session.voice_mode = !session.voice_mode;
            format!(
                "Voice mode {}",
                if session.voice_mode { "ON" } else { "OFF" }
            )
        }
        Command::Persona => handle_persona_command_text(msg.text().unwrap_or("/persona")),
        Command::Update => crate::commands::update::handle_update().await,
        Command::Code => {
            // In Telegram, /code starts a coding session for this chat
            let session_id = format!("tg-code-{}", chat_id);
            let workspace = crate::commands::code::ensure_workspace(&session_id)
                .unwrap_or_else(|e| {
                    tracing::warn!("Failed to create workspace: {}", e);
                    std::path::PathBuf::from("/tmp/auxloclaw-code")
                });
            let _ = crate::commands::code::init_workspace(&workspace);
            format!(
                "Coding session started.\nWorkspace: {}\n\nSend your coding task as the next message.",
                workspace.display()
            )
        }
        Command::New => {
            let session_id = format!("tg:{}", chat_id);
            state.agent.clear_session(&session_id).await;
            state.clear_session(chat_id).await;
            "New session started".to_string()
        }
    };

    send_markdown_message(&bot, chat_id, &response).await?;
    Ok(())
}

async fn handle_message(bot: Bot, msg: Message, state: Arc<TelegramState>) -> ResponseResult<()> {
    let chat_id: i64 = msg.chat.id.0;
    let text = msg.text().unwrap_or("");
    if text.is_empty() {
        return Ok(());
    }
    if text.trim_start().starts_with("/persona") {
        let response = handle_persona_command_text(text);
        send_markdown_message(&bot, chat_id, &response).await?;
        return Ok(());
    }

    bot.send_chat_action(ChatId(chat_id), ChatAction::Typing)
        .await?;
    let session = state.get_or_create_session(chat_id).await;
    let response = state
        .agent
        .process(text, Some(&format!("tg:{}", chat_id)))
        .await;
    state.update_session(chat_id, None).await;

    if session.voice_mode {
        let voice_response = format!("Voice mode response\n\n{}", response);
        send_markdown_message(&bot, chat_id, &voice_response).await?;
    } else {
        if let Err(err) = send_markdown_message(&bot, chat_id, &response).await {
            tracing::warn!("Telegram send error: {err:?}");
        }
    }
    Ok(())
}

async fn send_markdown_message(bot: &Bot, chat_id: i64, text: &str) -> ResponseResult<()> {
    for chunk in split_telegram_message(text, 3900) {
        let formatted = markdown_to_telegram(&chunk);
        bot.send_message(ChatId(chat_id), formatted)
            .parse_mode(teloxide::types::ParseMode::MarkdownV2)
            .await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
    Ok(())
}

fn split_telegram_message(text: &str, max_chars: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return vec![text.to_string()];
    }

    chars
        .chunks(max_chars)
        .map(|chunk| chunk.iter().collect::<String>())
        .collect()
}
