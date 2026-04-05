//! Persona command handler

use anyhow::Result;
use std::fs;
use std::path::PathBuf;

use crate::cli::PersonaCommands;
use crate::persona::{PersonaConfig, StyleConfig, ResponseLength, Tone, FormattingConfig};
use crate::config::AppConfig;

pub fn handle_persona(action: PersonaCommands) -> Result<()> {
    let config_path = get_config_path()?;
    
    match action {
        PersonaCommands::Show => {
            show_persona(&config_path)?;
        }
        PersonaCommands::Edit => {
            edit_persona(&config_path)?;
        }
        PersonaCommands::Name { name } => {
            set_name(&config_path, &name)?;
        }
        PersonaCommands::Behavior { text } => {
            set_behavior(&config_path, &text)?;
        }
        PersonaCommands::Style { length, tone, no_em_dashes, no_emojis } => {
            set_style(&config_path, length, tone, no_em_dashes, no_emojis)?;
        }
        PersonaCommands::Load { file } => {
            load_persona(&config_path, &file)?;
        }
        PersonaCommands::Save { output } => {
            save_persona(&config_path, output.as_deref())?;
        }
        PersonaCommands::Reset => {
            reset_persona(&config_path)?;
        }
    }
    
    Ok(())
}

fn get_config_path() -> Result<PathBuf> {
    dirs::home_dir()
        .map(|h| h.join(".auxloclaw/config.toml"))
        .ok_or_else(|| anyhow::anyhow!("Could not find config directory"))
}

fn show_persona(config_path: &PathBuf) -> Result<()> {
    let config = AppConfig::load(config_path.to_str().unwrap())?;
    let persona = &config.persona;
    
    println!("\n🎭 Current Persona\n");
    println!("  Name: {}", persona.name);
    println!();
    println!("  Behavior:");
    for line in persona.behavior.lines() {
        println!("    {}", line);
    }
    println!();
    println!("  Style:");
    println!("    Length: {:?}", persona.style.length);
    println!("    Tone: {:?}", persona.style.tone);
    println!("    No em dashes: {}", persona.style.formatting.no_em_dashes);
    println!("    No emojis: {}", persona.style.formatting.no_emojis);
    println!();
    
    Ok(())
}

fn edit_persona(config_path: &PathBuf) -> Result<()> {
    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| "nano".into());
    
    // Create PERSONA.md file
    let persona_path = config_path.parent()
        .ok_or_else(|| anyhow::anyhow!("Invalid config path"))?
        .join("PERSONA.md");
    
    if !persona_path.exists() {
        create_default_persona_md(&persona_path)?;
    }
    
    // Open editor
    std::process::Command::new(&editor)
        .arg(&persona_path)
        .status()?;
    
    println!("\n✓ Persona updated from {}", persona_path.display());
    println!("  Run 'auxloclaw persona show' to see changes\n");
    
    Ok(())
}

fn set_name(config_path: &PathBuf, name: &str) -> Result<()> {
    update_persona_field(config_path, |p| {
        p.name = name.to_string();
    })?;
    
    println!("\n✓ Persona name set to: {}\n", name);
    Ok(())
}

fn set_behavior(config_path: &PathBuf, text: &str) -> Result<()> {
    update_persona_field(config_path, |p| {
        p.behavior = text.to_string();
    })?;
    
    println!("\n✓ Persona behavior updated\n");
    Ok(())
}

fn set_style(
    config_path: &PathBuf,
    length: Option<String>,
    tone: Option<String>,
    no_em_dashes: bool,
    no_emojis: bool,
) -> Result<()> {
    update_persona_field(config_path, |p| {
        if let Some(l) = length {
            p.style.length = match l.to_lowercase().as_str() {
                "concise" => ResponseLength::Concise,
                "balanced" => ResponseLength::Balanced,
                "detailed" => ResponseLength::Detailed,
                _ => ResponseLength::default(),
            };
        }
        if let Some(t) = tone {
            p.style.tone = match t.to_lowercase().as_str() {
                "professional" => Tone::Professional,
                "casual" => Tone::Casual,
                "technical" => Tone::Technical,
                "friendly" => Tone::Friendly,
                _ => Tone::default(),
            };
        }
        if no_em_dashes {
            p.style.formatting.no_em_dashes = true;
        }
        if no_emojis {
            p.style.formatting.no_emojis = true;
        }
    })?;
    
    println!("\n✓ Persona style updated\n");
    Ok(())
}

fn load_persona(config_path: &PathBuf, file: &str) -> Result<()> {
    let persona_path = PathBuf::from(file);
    let persona = PersonaConfig::from_file(&persona_path)?;
    
    update_persona_field(config_path, |p| {
        *p = persona.clone();
    })?;
    
    println!("\n✓ Persona loaded from: {}\n", file);
    Ok(())
}

fn save_persona(config_path: &PathBuf, output: Option<&str>) -> Result<()> {
    let config = AppConfig::load(config_path.to_str().unwrap())?;
    let output_path = output
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            config_path.parent()
                .expect("Invalid config path")
                .join("PERSONA.md")
        });
    
    // Create PERSONA.md content
    let content = format!(
        "---\nname: {}\n---\n\n{}\n",
        config.persona.name,
        config.persona.behavior
    );
    
    fs::write(&output_path, content)?;
    
    println!("\n✓ Persona saved to: {}\n", output_path.display());
    Ok(())
}

fn reset_persona(config_path: &PathBuf) -> Result<()> {
    update_persona_field(config_path, |p| {
        *p = PersonaConfig::default();
    })?;
    
    println!("\n✓ Persona reset to defaults\n");
    Ok(())
}

fn update_persona_field<F>(config_path: &PathBuf, f: F) -> Result<()>
where
    F: FnOnce(&mut PersonaConfig),
{
    let mut config = AppConfig::load(config_path.to_str().unwrap())?;
    f(&mut config.persona);
    
    // Write back to config
    let content = toml::to_string_pretty(&config)?;
    fs::write(config_path, content)?;
    
    Ok(())
}

fn create_default_persona_md(path: &PathBuf) -> Result<()> {
    let content = r#"---
name: AUXLOCLAW
---

You are AUXLOCLAW, a high-performance AI agent.

## Capabilities

- File operations (read, write, edit)
- Code execution (sandboxed)
- Web search
- Skill-based workflows

## Behavior

Be helpful, concise, and accurate. Use tools when appropriate.
"#;
    
    fs::write(path, content)?;
    Ok(())
}