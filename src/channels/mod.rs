//! Channel adapters: Telegram, Discord, Slack, CLI

pub mod telegram;
pub mod discord;

// Re-export channel traits
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Message from a channel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMessage {
    pub channel: ChannelType,
    pub chat_id: String,
    pub user_id: String,
    pub content: String,
    pub metadata: serde_json::Value,
}

/// Message to a channel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundMessage {
    pub channel: ChannelType,
    pub chat_id: String,
    pub content: String,
    pub reply_to: Option<String>,
    pub metadata: serde_json::Value,
}

/// Supported channel types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ChannelType {
    Telegram,
    Discord,
    Slack,
    CLI,
    Web,
    WhatsApp,
}

/// Channel adapter trait
#[async_trait]
pub trait ChannelAdapter: Send + Sync {
    async fn send(&self, message: OutboundMessage) -> anyhow::Result<()>;
    async fn receive(&self) -> Option<InboundMessage>;
    fn channel_type(&self) -> ChannelType;
}