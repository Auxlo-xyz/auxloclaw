//! Chat command handler

use anyhow::Result;
use dialoguer::{theme::ColorfulTheme, Input, History};
use std::sync::Arc;

pub async fn handle_chat(
    message: Option<String>,
    model: Option<String>,
    stream: bool,
) -> Result<()> {
    // Load config
    let config_path = dirs::home_dir()
        .map(|h| h.join(".auxloclaw/config.toml"))
        .ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?;
    let config = crate::config::AppConfig::load(config_path.to_str().unwrap_or("~/.auxloclaw/config.toml"))?;
    
    // Initialize components
    let memory = Arc::new(crate::memory::MemoryEngine::new(&config.memory).await?);
    let providers = Arc::new(crate::providers::ProviderPool::new(config.providers.clone()));
    let orchestrator = Arc::new(crate::orchestrator::ToolOrchestrator::new());
    let agent = Arc::new(crate::agent::AgentCore::new(memory, providers, orchestrator, config.clone()));
    
    match message {
        Some(msg) => {
            // One-shot mode
            let response = agent.process(&msg).await;
            println!("{}", response);
        }
        None => {
            // Interactive mode
            println!("\n🦞 AUXLOCLAW Chat (type 'exit' to quit, 'help' for commands)\n");
            
            let mut history = dialoguer::BasicHistory::new();
            
            loop {
                let input: String = dialoguer::Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
                    .with_prompt("You")
                    .history_with(&mut history)
                    .interact_text()?;
                
                match input.trim() {
                    "exit" | "quit" | "q" => {
                        println!("Goodbye!");
                        break;
                    }
                    "help" | "?" => {
                        println!("\nCommands:");
                        println!("  exit, quit, q  - Exit chat");
                        println!("  help, ?        - Show this help");
                        println!("  clear          - Clear history");
                        println!("  status         - Show session status");
                        println!("  skill <name>   - Activate a skill");
                        println!();
                        continue;
                    }
                    "clear" => {
                        history = dialoguer::BasicHistory::new();
                        println!("History cleared.");
                        continue;
                    }
                    "status" => {
                        println!("\nSession Status:");
                        println!("  Model: {}", config.agent.default_model);
                        println!();
                        continue;
                    }
                    "" => continue,
                    _ => {}
                }
                
                // Process message
                let response = agent.process(&input).await;
                println!("\n{}\n", response);
            }
        }
    }
    
    Ok(())
}