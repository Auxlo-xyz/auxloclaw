//! Config command handler

use anyhow::{bail, Result};
use std::fs;

pub fn handle_config(action: crate::cli::ConfigCommands) -> Result<()> {
    let config_path = dirs::home_dir()
        .map(|h| h.join(".auxloclaw/config.toml"))
        .ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?;
    
    match action {
        crate::cli::ConfigCommands::Show { format } => {
            if !config_path.exists() {
                bail!("Config file not found. Run `auxloclaw setup` first.");
            }
            
            let content = fs::read_to_string(&config_path)?;
            
            match format.as_str() {
                "toml" => println!("{}", content),
                "json" => {
                    let value: toml::Value = toml::from_str(&content)?;
                    println!("{}", serde_json::to_string_pretty(&value)?);
                }
                _ => bail!("Unknown format: {}", format),
            }
        }
        
        crate::cli::ConfigCommands::Get { key } => {
            if !config_path.exists() {
                bail!("Config file not found. Run `auxloclaw setup` first.");
            }
            
            let content = fs::read_to_string(&config_path)?;
            let config: toml::Value = toml::from_str(&content)?;
            
            let parts: Vec<&str> = key.split('.').collect();
            let mut current = &config;
            
            for part in &parts[..parts.len()-1] {
                current = current.get(part)
                    .ok_or_else(|| anyhow::anyhow!("Key not found: {}", key))?;
            }
            
            let last = parts.last().unwrap();
            if let Some(value) = current.get(last) {
                match value {
                    toml::Value::String(s) => println!("{}", s),
                    toml::Value::Integer(i) => println!("{}", i),
                    toml::Value::Float(f) => println!("{}", f),
                    toml::Value::Boolean(b) => println!("{}", b),
                    _ => println!("{}", value),
                }
            } else {
                bail!("Key not found: {}", key);
            }
        }
        
        crate::cli::ConfigCommands::Set { key, value } => {
            if !config_path.exists() {
                bail!("Config file not found. Run `auxloclaw setup` first.");
            }
            
            // Simple implementation - just append/set the value
            println!("Setting {} = {}", key, value);
            println!("Note: Direct config editing not fully implemented. Use `auxloclaw config edit`.");
        }
        
        crate::cli::ConfigCommands::Edit => {
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".into());
            let status = std::process::Command::new(&editor)
                .arg(&config_path)
                .status()?;
            
            if status.success() {
                println!("Config updated.");
            } else {
                bail!("Editor exited with error");
            }
        }
        
        crate::cli::ConfigCommands::Reset { yes } => {
            if !yes {
                println!("This will reset all configuration to defaults.");
                let confirm = dialoguer::Confirm::new()
                    .with_prompt("Continue?")
                    .default(false)
                    .interact()?;
                
                if !confirm {
                    println!("Cancelled.");
                    return Ok(());
                }
            }
            
            if config_path.exists() {
                fs::remove_file(&config_path)?;
            }
            println!("Configuration reset. Run `auxloclaw setup` to configure.");
        }
        
        crate::cli::ConfigCommands::Validate => {
            if !config_path.exists() {
                bail!("Config file not found. Run `auxloclaw setup` first.");
            }
            
            let content = fs::read_to_string(&config_path)?;
            match toml::from_str::<crate::config::AppConfig>(&content) {
                Ok(_) => println!("✅ Configuration is valid."),
                Err(e) => bail!("Configuration error: {}", e),
            }
        }
    }
    
    Ok(())
}