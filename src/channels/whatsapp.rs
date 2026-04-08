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
        Self { agent, bridge_url }
    }

    pub async fn handle_message(&self, jid: &str, push_name: &str, text: &str) -> anyhow::Result<()> {
        tracing::info!("WhatsApp message from {}: {}", push_name, text);
        
        // Format session ID as wa:{jid}
        let session_id = format!("wa:{}", jid);
        
        // Process with agent
        let response = self.agent.process(text, Some(&session_id)).await;
        
        // Send response back via bridge
        self.send_message(jid, &response).await
    }

    pub async fn send_message(&self, jid: &str, text: &str) -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let res = client.post(format!("{}/send", self.bridge_url))
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

    pub async fn get_pairing_code(&self) -> anyhow::Result<String> {
        let client = reqwest::Client::new();
        let res = client.get(format!("{}/pairing-code", self.bridge_url))
            .send()
            .await?;
        
        if res.status().is_success() {
            let data: serde_json::Value = res.json().await?;
            Ok(data["code"].as_str().unwrap_or("Error retrieving code").to_string())
        } else {
            Err(anyhow!("Bridge returned error: {}", res.status()))
        }
    }
}
