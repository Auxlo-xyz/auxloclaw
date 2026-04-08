//! Configuration management for AUXLOCLAW

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::fs;
use anyhow::{Context, Result};

use crate::persona::PersonaConfig;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct AppConfig {
    pub agent: AgentConfig,
    pub persona: PersonaConfig,
    pub providers: ProvidersConfig,
    pub memory: MemoryConfig,
    pub channels: ChannelsConfig,
    pub tools: ToolsConfig,
    pub server: ServerConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentConfig {
    pub name: String,
    pub default_model: String,
    pub max_tokens: u32,
    pub temperature: f32,
    pub max_tool_iterations: u32,
    pub context_window_tokens: u32,
    pub timezone: String,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            name: "AUXLOCLAW".into(),
            default_model: "nvidia/llama-3.1-nemotron-70b-instruct".into(),
            max_tokens: 8192,
            temperature: 1.0,
            max_tool_iterations: 50,
            context_window_tokens: 20000,
            timezone: "UTC".into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProvidersConfig {
    pub primary: ProviderEntry,
    #[serde(default)]
    pub fallbacks: Vec<ProviderEntry>,
    #[serde(default = "default_pool_size")]
    pub connection_pool_size: usize,
    #[serde(default = "default_timeout")]
    pub request_timeout_secs: u64,
}

fn default_pool_size() -> usize { 32 }
fn default_timeout() -> u64 { 60 }

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProviderEntry {
    pub name: String,
    pub api_key: String,
    pub api_base: String,
    #[serde(default)]
    pub extra_headers: Option<HashMap<String, String>>,
}

impl Default for ProvidersConfig {
    fn default() -> Self {
        Self {
            primary: ProviderEntry {
                name: "nvidia".into(),
                api_key: String::new(),
                api_base: "https://integrate.api.nvidia.com/v1".into(),
                extra_headers: None,
            },
            fallbacks: vec![],
            connection_pool_size: 32,
            request_timeout_secs: 60,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MemoryConfig {
    #[serde(default = "default_cache_size")]
    pub hot_cache_size: usize,
    #[serde(default = "default_db_path")]
    pub database_path: String,
    #[serde(default)]
    pub embedding_model: Option<String>,
    #[serde(default = "default_consolidation")]
    pub consolidation_interval_secs: u64,
}

fn default_cache_size() -> usize { 1000 }
fn default_db_path() -> String { "~/.auxloclaw/memory.db".into() }
fn default_consolidation() -> u64 { 300 }

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            hot_cache_size: 1000,
            database_path: "~/.auxloclaw/memory.db".into(),
            embedding_model: None,
            consolidation_interval_secs: 300,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct ChannelsConfig {
    pub telegram: TelegramConfig,
    pub discord: DiscordConfig,
    pub slack: SlackConfig,
    pub whatsapp: WhatsAppConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct WhatsAppConfig {
    pub enabled: bool,
    pub phone_number: String,
    pub auth_dir: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct TelegramConfig {
    pub enabled: bool,
    pub token: String,
    pub webhook_url: Option<String>,
    pub allowed_users: Vec<String>,
    pub group_policy: GroupPolicy,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct DiscordConfig {
    pub enabled: bool,
    pub token: String,
    pub allowed_guilds: Vec<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct SlackConfig {
    pub enabled: bool,
    pub bot_token: String,
    pub app_token: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum GroupPolicy {
    #[serde(rename = "mention")]
    Mention,
    #[serde(rename = "open")]
    Open,
    #[serde(rename = "closed")]
    Closed,
}

impl Default for GroupPolicy {
    fn default() -> Self { Self::Mention }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolsConfig {
    #[serde(default = "default_true")]
    pub exec_enabled: bool,
    #[serde(default = "default_exec_timeout")]
    pub exec_timeout_secs: u64,
    #[serde(default = "default_true")]
    pub restrict_to_workspace: bool,
    #[serde(default)]
    pub web_search_enabled: bool,
    #[serde(default = "default_brave")]
    pub web_search_provider: String,
    #[serde(default)]
    pub web_search_api_key: Option<String>,
}

fn default_true() -> bool { true }
fn default_exec_timeout() -> u64 { 60 }
fn default_brave() -> String { "brave".into() }

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            exec_enabled: true,
            exec_timeout_secs: 60,
            restrict_to_workspace: true,
            web_search_enabled: true,
            web_search_provider: "brave".into(),
            web_search_api_key: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_true")]
    pub cors_enabled: bool,
}

fn default_host() -> String { "0.0.0.0".into() }
fn default_port() -> u16 { 18789 }

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".into(),
            port: 18789,
            cors_enabled: true,
        }
    }
}

impl AppConfig {
    pub fn load(path: &str) -> Result<Self> {
        // Try to load from file, otherwise use defaults with env overrides
        if Path::new(path).exists() {
            let content = fs::read_to_string(path)
                .with_context(|| format!("Failed to read config file: {}", path))?;
            let config: AppConfig = toml::from_str(&content)
                .with_context(|| "Failed to parse config file")?;
            return Ok(config.with_env_overrides());
        }

        // Create default config with env overrides
        Ok(Self::default().with_env_overrides())
    }

    fn with_env_overrides(mut self) -> Self {
        // Provider API keys from environment
        if let Ok(key) = std::env::var("NVIDIA_API_KEY") {
            self.providers.primary.api_key = key;
            self.providers.primary.name = "nvidia".into();
            self.providers.primary.api_base = "https://integrate.api.nvidia.com/v1".into();
        }
        if let Ok(key) = std::env::var("OPENAI_API_KEY") {
            if self.providers.primary.api_key.is_empty() {
                self.providers.primary.api_key = key;
                self.providers.primary.name = "openai".into();
                self.providers.primary.api_base = "https://api.openai.com/v1".into();
            }
        }
        if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
            if self.providers.primary.api_key.is_empty() {
                self.providers.primary.api_key = key;
                self.providers.primary.name = "anthropic".into();
                self.providers.primary.api_base = "https://api.anthropic.com/v1".into();
            }
        }

        // Telegram token from env
        if let Ok(token) = std::env::var("TELEGRAM_BOT_TOKEN") {
            self.channels.telegram.token = token.clone();
            self.channels.telegram.enabled = !token.is_empty();
        }

        // Discord token from env
        if let Ok(token) = std::env::var("DISCORD_BOT_TOKEN") {
            self.channels.discord.token = token.clone();
            self.channels.discord.enabled = !token.is_empty();
        }

        // WhatsApp phone number from env
        if let Ok(phone) = std::env::var("WHATSAPP_PHONE_NUMBER") {
            self.channels.whatsapp.phone_number = phone.clone();
            self.channels.whatsapp.enabled = !phone.is_empty();
        }

        // Memory database path
        if let Ok(path) = std::env::var("AUXLOCLAW_DB_PATH") {
            self.memory.database_path = path;
        }

        self
    }
}