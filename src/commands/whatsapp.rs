
use anyhow::{Result, anyhow};
use crate::config::AppConfig;
use crate::channels::whatsapp::WhatsAppState;
use std::sync::Arc;
use reqwest::Client;

pub async fn handle_whatsapp(action: crate::cli::WhatsAppCommands) -> Result<()> {
    // Load config to get the phone number and bridge URL
    let config_path = dirs::home_dir()
        .map(|h| h.join(".auxloclaw/config.toml"))
        .ok_or_else(|| anyhow!("Could not find config directory"))?;
    let config = AppConfig::load(config_path.to_str().unwrap_or("~/.auxloclaw/config.toml"))?;
    
    if !config.channels.whatsapp.enabled {
        return Err(anyhow!("WhatsApp is disabled in config.toml. Set [channels.whatsapp] enabled = true"));
    }

    let bridge_url = "http://localhost:18790".to_string();
    // We don't need the full agent here, just the bridge interface
    // However, WhatsAppState requires AgentCore. Since we only need the bridge API for these commands,
    // we can use a simpler approach or a dummy agent if needed. 
    // For now, let's use the bridge_url directly since we are doing simple HTTP calls.

    match action {
        crate::cli::WhatsAppCommands::Pair => {
            let phone = &config.channels.whatsapp.phone_number;
            if phone.is_empty() {
                return Err(anyhow!("Phone number not set in config.toml. Please set [channels.whatsapp] phone_number = \"your_number\""));
            }

            tracing::info!("Requesting pairing code for {}...", phone);
            let client = Client::new();
            let res = client.get(format!("{}/pairing-code?phone={}", bridge_url, phone))
                .send()
                .await?;
            
            if res.status().is_success() {
                let data: serde_json::Value = res.json().await?;
                let code = data["code"].as_str().unwrap_or("Error retrieving code");
                println!("\n=================================");
                println!("WhatsApp Pairing Code: {}", code);
                println!("=================================");
                println!("\n1. Open WhatsApp on your phone");
                println!("2. Go to Settings > Linked Devices");
                println!("3. Tap 'Link with phone number instead'");
                println!("4. Enter the code above\n");
                Ok(())
            } else {
                Err(anyhow!("Bridge returned error: {}", res.status()))
            }
        }
        crate::cli::WhatsAppCommands::Status => {
            let client = Client::new();
            let res = client.get(format!("{}/status", bridge_url))
                .send()
                .await?;
            
            if res.status().is_success() {
                let data: serde_json::Value = res.json().await?;
                let connected = data["connected"].as_bool().unwrap_or(false);
                if connected {
                    println!("✅ WhatsApp is connected and ready!");
                } else {
                    println!("❌ WhatsApp is not connected. Run 'auxloclaw whatsapp pair' to link your device.");
                }
                Ok(())
            } else {
                Err(anyhow!("Bridge returned error: {}", res.status()))
            }
        }
    }
}
