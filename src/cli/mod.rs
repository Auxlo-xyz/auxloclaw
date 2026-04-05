//! AUXLOCLAW CLI - User-friendly command interface

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "auxloclaw")]
#[command(author, version, about, long_about = None)]
#[command(next_line_help = true)]
pub struct Cli {
    /// Path to config file
    #[arg(short, long, global = true, default_value = "~/.auxloclaw/config.toml")]
    pub config: PathBuf,

    /// Enable debug logging
    #[arg(short, long, global = true)]
    pub debug: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the gateway server (Telegram, Discord, HTTP API)
    Gateway {
        /// Port to listen on
        #[arg(short, long, default_value = "18789")]
        port: u16,
        
        /// Host to bind to
        #[arg(long, default_value = "0.0.0.0")]
        host: String,
    },

    /// Chat with AUXLOCLAW (one-shot or interactive)
    Chat {
        /// The message to send (if not provided, enters interactive mode)
        message: Option<String>,
        
        /// Model to use
        #[arg(short, long)]
        model: Option<String>,
        
        /// Stream the response
        #[arg(short, long)]
        stream: bool,
    },

    /// Interactive setup wizard
    Setup {
        /// Non-interactive mode with defaults
        #[arg(short, long)]
        quick: bool,
        
        /// Enable Telegram
        #[arg(long)]
        telegram: bool,
        
        /// Enable Discord
        #[arg(long)]
        discord: bool,
    },

    /// Manage configuration
    Config {
        #[command(subcommand)]
        action: ConfigCommands,
    },

    /// Manage skills
    Skill {
        #[command(subcommand)]
        action: SkillCommands,
    },

    /// Manage providers
    Provider {
        #[command(subcommand)]
        action: ProviderCommands,
    },

    /// Show system status
    Status,

    /// Run a skill
    Run {
        /// Skill name
        skill: String,
        
        /// Arguments for the skill
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
}

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Show current configuration
    Show {
        /// Output format
        #[arg(short, long, default_value = "toml")]
        format: String,
    },

    /// Set a configuration value
    Set {
        /// Configuration key (e.g., "agent.temperature")
        key: String,
        
        /// Configuration value
        value: String,
    },

    /// Get a configuration value
    Get {
        /// Configuration key
        key: String,
    },

    /// Edit configuration in editor
    Edit,

    /// Reset to defaults
    Reset {
        /// Confirm reset
        #[arg(short, long)]
        yes: bool,
    },

    /// Validate configuration
    Validate,
}

#[derive(Subcommand)]
pub enum SkillCommands {
    /// List all available skills
    List {
        /// Filter by category
        #[arg(short, long)]
        category: Option<String>,
        
        /// Show details
        #[arg(short, long)]
        detailed: bool,
    },

    /// Install a skill from registry
    Install {
        /// Skill name or URL
        skill: String,
        
        /// Force reinstall
        #[arg(short, long)]
        force: bool,
    },

    /// Create a new skill
    Create {
        /// Skill name
        name: String,
        
        /// Category
        #[arg(short, long, default_value = "custom")]
        category: String,
        
        /// Open in editor after creation
        #[arg(short, long)]
        edit: bool,
    },

    /// Show skill details
    Show {
        /// Skill name
        skill: String,
    },

    /// Edit a skill
    Edit {
        /// Skill name
        skill: String,
    },

    /// Delete a skill
    Delete {
        /// Skill name
        skill: String,
        
        /// Confirm deletion
        #[arg(short, long)]
        yes: bool,
    },

    /// Search skills
    Search {
        /// Search query
        query: String,
    },

    /// Update all skills from registry
    Update {
        /// Update specific skill
        skill: Option<String>,
    },

    /// Validate a skill
    Validate {
        /// Skill name
        skill: String,
    },
}

#[derive(Subcommand)]
pub enum ProviderCommands {
    /// List all providers
    List,

    /// Set primary provider
    Set {
        /// Provider name
        name: String,
    },

    /// Test provider connection
    Test {
        /// Provider name (tests all if not specified)
        name: Option<String>,
    },

    /// Add a new provider
    Add {
        /// Provider name
        name: String,
        
        /// API base URL
        #[arg(long)]
        base: String,
        
        /// API key (will prompt if not provided)
        #[arg(short, long)]
        key: Option<String>,
    },

    /// Remove a provider
    Remove {
        /// Provider name
        name: String,
    },
}

impl Cli {
    pub fn parse_args() -> Self {
        Self::parse()
    }
}