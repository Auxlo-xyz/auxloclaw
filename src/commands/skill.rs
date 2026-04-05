//! Skill command handler

use anyhow::{bail, Result};
use std::fs;
use std::path::PathBuf;

pub fn handle_skill(action: crate::cli::SkillCommands) -> Result<()> {
    let skills_dir = dirs::home_dir()
        .map(|h| h.join(".auxloclaw/skills"))
        .ok_or_else(|| anyhow::anyhow!("Could not find skills directory"))?;
    
    match action {
        crate::cli::SkillCommands::List { category, detailed } => {
            if !skills_dir.exists() {
                println!("No skills installed. Run `auxloclaw skill install <name>` to add skills.");
                return Ok(());
            }
            
            println!("\n📚 Installed Skills\n");
            
            let mut count = 0;
            for entry in walkdir::WalkDir::new(&skills_dir)
                .min_depth(2)
                .max_depth(3)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_name() == "SKILL.md")
            {
                let skill_path = entry.path();
                let content = fs::read_to_string(skill_path)?;
                let skill: crate::skills::Skill = crate::skills::Skill::parse(&content)?;
                
                // Filter by category if specified
                if let Some(ref cat) = category {
                    let skill_category = skill_path
                        .strip_prefix(&skills_dir)?
                        .iter()
                        .next()
                        .map(|s| s.to_string_lossy())
                        .unwrap_or_default();
                    if !skill_category.to_lowercase().contains(&cat.to_lowercase()) {
                        continue;
                    }
                }
                
                if detailed {
                    println!("━━━ {} ━━━", skill.name());
                    println!("    {}", skill.description());
                    println!("    Path: {}", skill_path.display());
                    println!();
                } else {
                    let desc = skill.description();
                    let truncated = desc.chars().take(60).collect::<String>();
                    let suffix = if desc.len() > 60 { "..." } else { "" };
                    println!("  {} - {}{}", skill.name(), truncated, suffix);
                }
                count += 1;
            }
            
            println!("\n{} skills found.", count);
        }
        
        crate::cli::SkillCommands::Install { skill, force } => {
            println!("Installing skill: {}", skill);
            
            // Check if it's a URL or name
            if skill.starts_with("http") {
                // Download from URL
                println!("Downloading from URL...");
                // TODO: Implement URL download
            } else {
                // Search in registry
                println!("Searching in registry...");
                // TODO: Implement registry search
            }
            
            if !force && skills_dir.join(&skill).exists() {
                let confirm = dialoguer::Confirm::new()
                    .with_prompt("Skill already exists. Overwrite?")
                    .default(false)
                    .interact()?;
                if !confirm {
                    println!("Cancelled.");
                    return Ok(());
                }
            }
            
            println!("✓ Skill '{}' installed.", skill);
        }
        
        crate::cli::SkillCommands::Create { name, category, edit } => {
            fs::create_dir_all(skills_dir.join(&category).join(&name))?;
            
            let skill_content = format!(r#"---
name: {}
description: A new skill for {}
compatibility: Created for AUXLOCLAW
metadata:
  author: {}
---

# {}

Instructions for this skill go here.

## Usage

Describe how to use this skill.

## Examples

Provide examples of common use cases.
"#,
                name,
                name,
                std::env::var("USER").unwrap_or_else(|_| "user".into()),
                name
            );
            
            let skill_path = skills_dir.join(&category).join(&name).join("SKILL.md");
            fs::write(&skill_path, &skill_content)?;
            
            println!("✓ Created skill: {}", name);
            println!("  Path: {}", skill_path.display());
            
            if edit {
                let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".into());
                std::process::Command::new(&editor)
                    .arg(&skill_path)
                    .status()?;
            }
        }
        
        crate::cli::SkillCommands::Show { skill } => {
            let skill_path = find_skill(&skills_dir, &skill)?;
            let content = fs::read_to_string(&skill_path)?;
            println!("{}", content);
        }
        
        crate::cli::SkillCommands::Edit { skill } => {
            let skill_path = find_skill(&skills_dir, &skill)?;
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".into());
            std::process::Command::new(&editor)
                .arg(&skill_path)
                .status()?;
            println!("Skill updated.");
        }
        
        crate::cli::SkillCommands::Delete { skill, yes } => {
            let skill_path = find_skill(&skills_dir, &skill)?;
            let skill_dir = skill_path.parent().unwrap();
            
            if !yes {
                println!("This will delete the skill: {}", skill);
                let confirm = dialoguer::Confirm::new()
                    .with_prompt("Continue?")
                    .default(false)
                    .interact()?;
                if !confirm {
                    println!("Cancelled.");
                    return Ok(());
                }
            }
            
            fs::remove_dir_all(skill_dir)?;
            println!("✓ Skill '{}' deleted.", skill);
        }
        
        crate::cli::SkillCommands::Search { query } => {
            println!("Searching for: {}", query);
            // TODO: Implement registry search
            println!("Registry search not implemented yet.");
        }
        
        crate::cli::SkillCommands::Update { skill } => {
            match skill {
                Some(name) => {
                    println!("Updating skill: {}", name);
                    // TODO: Implement update
                }
                None => {
                    println!("Updating all skills...");
                    // TODO: Implement bulk update
                }
            }
        }
        
        crate::cli::SkillCommands::Validate { skill } => {
            let skill_path = find_skill(&skills_dir, &skill)?;
            let content = fs::read_to_string(&skill_path)?;
            
            match crate::skills::Skill::parse(&content) {
                Ok(s) => {
                    println!("✅ Skill '{}' is valid.", s.name());
                    println!("   Description: {}", s.description());
                }
                Err(e) => {
                    bail!("Invalid skill: {}", e);
                }
            }
        }
    }
    
    Ok(())
}

fn find_skill(skills_dir: &PathBuf, name: &str) -> Result<PathBuf> {
    for entry in walkdir::WalkDir::new(skills_dir)
        .min_depth(2)
        .max_depth(3)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name() == "SKILL.md")
    {
        let path = entry.path();
        let content = fs::read_to_string(path)?;
        if let Ok(skill) = crate::skills::Skill::parse(&content) {
            if skill.name() == name {
                return Ok(path.to_path_buf());
            }
        }
    }
    
    bail!("Skill '{}' not found", name)
}