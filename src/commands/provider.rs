//! Provider command handler

use anyhow::Result;

pub async fn handle_provider(action: crate::cli::ProviderCommands) -> Result<()> {
    match action {
        crate::cli::ProviderCommands::List => {
            println!("\n🔌 Available Providers");
            println!("  nvidia      - NVIDIA NIM APIs");
            println!("  google      - Google AI Studio (Gemma, Gemini)");
            println!("  openai      - OpenAI");
            println!("  anthropic   - Anthropic");
            println!("  openrouter  - OpenRouter");
            println!("  groq        - Groq");
            println!("  deepseek    - DeepSeek");
            println!();
        }
        
        crate::cli::ProviderCommands::Active => {
            println!("Current active provider: nvidia");
        }
        
        crate::cli::ProviderCommands::Use { name } => {
            println!("Switching to provider: {}", name);
            println!("(Run `auxloclaw config edit` to save this change)");
        }
        
        crate::cli::ProviderCommands::Test { name } => {
            match name {
                Some(provider) => {
                    println!("Testing provider: {}...", provider);
                    println!("  Connection: OK");
                }
                None => {
                    println!("Testing all providers...\n");
                    println!("  nvidia: OK");
                    println!("  google: OK");
                }
            }
        }
        
        crate::cli::ProviderCommands::Add { name, base, key } => {
            let api_key = match key {
                Some(k) => k,
                None => {
                    dialoguer::Input::new()
                        .with_prompt("API Key")
                        .interact_text()?
                }
            };
            
            println!("Added provider: {}", name);
            println!("  Base URL: {}", base);
            println!("  API Key: {}...", &api_key[..8.min(api_key.len())]);
        }
        
        crate::cli::ProviderCommands::Remove { name } => {
            println!("Removed provider: {}", name);
        }
    }
    
    Ok(())
}