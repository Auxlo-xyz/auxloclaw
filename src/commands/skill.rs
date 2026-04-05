//! Skill command handler

use anyhow::{bail, Result};
use std::fs;
use std::path::PathBuf;

use crate::skills::{SkillInstaller, registry::SkillRegistry};

pub async fn handle_skill(action: crate::cli::SkillCommands) -> Result<()> {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("auxloclaw");
    
    let skills_dir = config_dir.join("skills");
    fs::create_dir_all(&skills_dir)?;

    let mut installer = SkillInstaller::new(skills_dir.clone());

    match action {
        crate::cli::SkillCommands::List { detailed } => {
            println!("\n📚 Installed Skills\n");
            
            let mut count = 0;
            for entry in walkdir::WalkDir::new(&skills_dir)
                .min_depth(2)
                .max_depth(3)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                if entry.file_name() == "SKILL.md" {
                    if let Ok(content) = fs::read_to_string(entry.path()) {
                        if let Ok(skill) = crate::skills::Skill::parse(&content) {
                            if detailed {
                                println!("{}", "─".repeat(50));
                                println!("📦 {}", skill.name());
                                println!("   {}", skill.description());
                                if let Some(cat) = &skill.meta.category {
                                    println!("   Category: {}", cat);
                                }
                                println!();
                            } else {
                                println!("  {} - {}", skill.name(), 
                                    if skill.description().len() > 60 {
                                        format!("{}...", &skill.description()[..57])
                                    } else {
                                        skill.description().to_string()
                                    }
                                );
                            }
                            count += 1;
                        }
                    }
                }
            }
            
            println!("\n{} skills found.\n", count);
        }

        crate::cli::SkillCommands::Search { query } => {
            println!("\n🔍 Searching registry for: '{}'\n", query);
            
            let results: Vec<crate::skills::registry::RegistrySkill> = installer.search(&query).await?;
            
            if results.is_empty() {
                println!("No skills found matching '{}'\n", query);
                println!("💡 Try different keywords or browse all: auxloclaw skill browse");
            } else {
                for skill in &results {
                    let installed = if installer.is_installed(&skill.name) {
                        "✓"
                    } else {
                        " "
                    };
                    println!("  [{}] {} - {}", installed, skill.name, skill.description);
                }
                println!("\n{} results. Install with: auxloclaw skill install <name>\n", results.len());
            }
        }

        crate::cli::SkillCommands::Install { name, url, git } => {
            println!();
            
            if let Some(url) = url {
                println!("📥 Installing from URL: {}", url);
                match installer.install_from_url(&url).await {
                    Ok(skill_name) => {
                        println!("✓ Installed skill: {}", skill_name);
                    }
                    Err(e) => {
                        println!("✗ Failed to install: {}", e);
                    }
                }
            } else if let Some(git_url) = git {
                println!("📥 Installing from git: {}", git_url);
                match installer.install_from_git(&git_url) {
                    Ok(skills) => {
                        println!("✓ Installed: {}", skills);
                    }
                    Err(e) => {
                        println!("✗ Failed to install: {}", e);
                    }
                }
            } else if let Some(skill_name) = name {
                println!("📥 Installing skill: {}", skill_name);
                match installer.install(&skill_name).await {
                    Ok(msg) => {
                        println!("✓ {}", msg);
                    }
                    Err(e) => {
                        println!("✗ Failed: {}", e);
                    }
                }
            } else {
                println!("✗ Please specify a skill name, --url, or --git\n");
            }
            println!();
        }

        crate::cli::SkillCommands::Uninstall { name } => {
            println!();
            if installer.is_installed(&name) {
                installer.uninstall(&name)?;
                println!("✓ Uninstalled skill: {}\n", name);
            } else {
                println!("✗ Skill '{}' is not installed\n", name);
            }
        }

        crate::cli::SkillCommands::Create { name, description } => {
            println!();
            let desc = description.as_deref().unwrap_or("A custom skill");
            let skill_dir = installer.create_from_template(&name, desc)?;
            println!("✓ Created skill '{}' at:\n  {:?}\n", name, skill_dir);
            println!("Edit the SKILL.md file to add your instructions.\n");
        }

        crate::cli::SkillCommands::Update { name } => {
            println!();
            if installer.is_installed(&name) {
                // Re-install from registry
                match installer.install(&name).await {
                    Ok(msg) => println!("✓ {}\n", msg),
                    Err(e) => println!("✗ Failed to update: {}\n", e),
                }
            } else {
                println!("✗ Skill '{}' is not installed\n", name);
            }
        }

        crate::cli::SkillCommands::Browse => {
            println!("\n🌐 Available Skills in Registry\n");
            
            let skills = installer.list_available().await?;
            
            // Group by category
            let mut by_category: std::collections::HashMap<String, Vec<_>> = std::collections::HashMap::new();
            for skill in skills {
                by_category
                    .entry(skill.category.clone())
                    .or_default()
                    .push(skill);
            }

            for (category, skills) in by_category {
                println!("\n📁 {}", category);
                for skill in skills {
                    let installed = if installer.is_installed(&skill.name) { "✓" } else { " " };
                    println!("  [{}] {} - {}", installed, skill.name, 
                        if skill.description.len() > 50 {
                            format!("{}...", &skill.description[..47])
                        } else {
                            skill.description.clone()
                        }
                    );
                }
            }

            println!("\n💡 Install with: auxloclaw skill install <name>\n");
        }

        crate::cli::SkillCommands::Info { name } => {
            println!();
            
            let skill_path = find_skill(&skills_dir, &name)?;
            let content = fs::read_to_string(&skill_path)?;
            let skill = crate::skills::Skill::parse(&content)?;

            println!("📦 {}\n", skill.name());
            println!("{}\n", skill.description());
            
            if let Some(cat) = &skill.meta.category {
                println!("Category: {}", cat);
            }
            if let Some(license) = &skill.meta.license {
                println!("License: {}", license);
            }
            if let Some(compat) = &skill.meta.compatibility {
                println!("Compatibility: {}", compat);
            }

            println!("\n{}\n", "─".repeat(50));
            println!("{}\n", skill.body);
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
    {
        if entry.file_name() == "SKILL.md" {
            if let Ok(content) = fs::read_to_string(entry.path()) {
                if let Ok(skill) = crate::skills::Skill::parse(&content) {
                    if skill.name() == name {
                        return Ok(entry.path().to_path_buf());
                    }
                }
            }
        }
    }
    
    bail!("Skill '{}' not found", name)
}