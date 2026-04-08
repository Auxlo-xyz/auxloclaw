
//! WhatsApp channel adapter via TypeScript bridge

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::agent::AgentCore;
use crate::channels::markdown::markdown_to_telegram; // Reusing markdown formatting if applicable, or creating a generic one

#[derive(Debug, Clone, Default)]
struct SessionState {
    message_count: u64,
    total_tokens: u64,
    created_at: chrono::DateTime<chrono::Utc>,
    last_activity: chrono::DateTime<chrono::Utc>,
}

pub struct WhatsAppState {
    agent: Arc<AgentCore>,
    bridge_url: String,
}

impl WhatsAppState {
    pub fn new(agent: Arc<AgentCore>, bridge_url: String) -> Self {
        Self {
            agent,
            bridge_url,
        }
    }

    async fn send_to_bridge(&self, jid: &str, text: &str) -> Result<()> {
        let client = reqwest::Client::new();
        let res = client
            .post(format!("{}/send", self.bridge_url))
            .json(&serde_json::json!({
                "jid": jid,
                "text": text
            }))
            .send()
            .await?;

        if res.status().is_success() {
            Ok(())
        } else {
            Err(anyhow!("Bridge returned error: {}", res.status()))
        }
    }

    pub async fn handle_message(&self, jid: &str, push_name: &str, text: &str) -> Result<()> {
        tracing::info!("📱 WhatsApp message from {} ({}): {}", push_name, jid, text);

        // session_id for WhatsApp is wa:{jid}
        let session_id = format!("wa:{}", jid);

        // Process message with agent
        // AgentCore::process takes text and an optional session_id (or just uses it internally)
        // Looking at telegram.rs, it does: agent.process(text, Some(chat_id))
        // But agent.process in AgentCore likely expects an Option<String> or similar for session.
        // Let's check AgentCore::process signature.
        let response = self.agent.process(text, Some(&session_id)).await;

        // Formatting: WhatsApp uses a similar markdown style to Telegram (bold *, italic _, etc.)
        // For now, we'll send the raw response or use a simple formatter.
        let formatted_response = response; 

        // Send back via bridge
        self.send_to_bridge(jid, &formatted_response).await?;

        Ok(())
    }
}
