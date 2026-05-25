//! Setup wizard

use anyhow::{bail, Result};
use dialoguer::{theme::ColorfulTheme, Input, Select, Confirm};
use std::fs;
use std::path::PathBuf;

pub fn handle_setup(quick: bool, telegram: bool, discord: bool) -> Result<()> {
    println!("\nAUXLOCLAW Setup Wizard\n");
    
    let config_dir = dirs::home_dir()
        .map(|h| h.join(".auxloclaw"))
        .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    
    let config_path = config_dir.join("config.toml");
    
    if quick {
        return quick_setup(&config_dir, telegram, discord);
    }
    
    // Interactive setup
    println!("This wizard will help you configure AUXLOCLAW.\n");
    
    // Create config directory
    if !config_dir.exists() {
        fs::create_dir_all(&config_dir)?;
        fs::create_dir_all(config_dir.join("skills"))?;
        fs::create_dir_all(config_dir.join("memory"))?;
        println!("Created config directory: {:?}", config_dir);
    }
    
    // Agent name
    let agent_name: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Agent name")
        .default("AUXLOCLAW".into())
        .interact_text()?;
    
    // Provider selection
    let providers = vec![
        "NVIDIA (stepfun-ai/step-3.5-flash)",
        "OpenAI (gpt-4)",
        "Anthropic (claude-3-opus)",
        "OpenRouter (multi-model)",
        "Groq (llama-3.1)",
        "Custom endpoint",
    ];
    
    let provider_idx = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select your LLM provider")
        .items(&providers)
        .default(0)
        .interact()?;
    
    let (provider_name, api_base, model) = match provider_idx {
        0 => ("nvidia", "https://integrate.api.nvidia.com/v1".to_string(), "stepfun-ai/step-3.5-flash".to_string()),
        1 => ("openai", "https://api.openai.com/v1".to_string(), "gpt-4-turbo".to_string()),
        2 => ("anthropic", "https://api.anthropic.com/v1".to_string(), "claude-3-opus-20240229".to_string()),
        3 => ("openrouter", "https://openrouter.ai/api/v1".to_string(), "anthropic/claude-3-opus".to_string()),
        4 => ("groq", "https://api.groq.com/openai/v1".to_string(), "llama-3.1-70b-versatile".to_string()),
        5 => {
            let base: String = Input::with_theme(&ColorfulTheme::default())
                .with_prompt("API base URL")
                .interact_text()?;
            ("custom", base, "custom-model".to_string())
        },
        _ => bail!("Invalid selection"),
    };
    
    // API Key
    let api_key: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("API Key")
        .interact_text()?;
    
    // Temperature
    let temp_str: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Temperature (0.0-2.0)")
        .default("1.0".into())
        .interact_text()?;
    let temperature: f32 = temp_str.parse().unwrap_or(1.0);
    
    // Channels
    let enable_telegram = telegram || Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Enable Telegram?")
        .default(false)
        .interact()?;
    
    let telegram_token = if enable_telegram {
        let token: String = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Telegram Bot Token")
            .allow_empty(true)
            .interact_text()?;
        Some(token)
    } else {
        None
    };
    
    let enable_discord = discord || Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Enable Discord?")
        .default(false)
        .interact()?;
    
    let discord_token = if enable_discord {
        let token: String = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Discord Bot Token")
            .allow_empty(true)
            .interact_text()?;
        Some(token)
    } else {
        None
    };
    
    // MCP Integrations
    let enable_github_mcp = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Enable GitHub integration (MCP)?")
        .default(false)
        .interact()?;
    
    let github_token = if enable_github_mcp {
        println!("  To create a token: GitHub Settings > Developer settings > Personal access tokens");
        println!("  Required scopes: repo, read:org, read:user\n");
        let token: String = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("GitHub Personal Access Token")
            .interact_text()?;
        Some(token)
    } else {
        None
    };
    
    // Extra MCP servers
    let mut extra_mcp_servers: Vec<(String, String, Vec<String>)> = Vec::new();
    if !enable_github_mcp {
        let add_more_mcp = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Add other MCP servers? (You can also use /mcp add later)")
            .default(false)
            .interact()?;
        
        if add_more_mcp {
            loop {
                println!("\nAdd MCP server (leave name empty to finish):");
                let name: String = Input::with_theme(&ColorfulTheme::default())
                    .with_prompt("  Server name (e.g. filesystem, slack)")
                    .allow_empty(true)
                    .interact_text()?;
                
                if name.is_empty() {
                    break;
                }
                
                let command: String = Input::with_theme(&ColorfulTheme::default())
                    .with_prompt("  Command (e.g. npx -y @modelcontextprotocol/server-filesystem)")
                    .interact_text()?;
                
                let args_str: String = Input::with_theme(&ColorfulTheme::default())
                    .with_prompt("  Arguments (space-separated, e.g. /home/workspace)")
                    .allow_empty(true)
                    .interact_text()?;
                
                let args: Vec<String> = if args_str.is_empty() {
                    Vec::new()
                } else {
                    args_str.split_whitespace().map(String::from).collect()
                };
                
                extra_mcp_servers.push((name, command, args));
            }
        }
    }
    
    // Token management
    if github_token.is_none() {
        println!("\nYou can set tokens later with:");
        println!("  auxloclaw token set GITHUB_TOKEN <your-token>");
        println!("  Or use /token set GITHUB_TOKEN <your-token> in Telegram/Discord\n");
    }
    
    // Generate config
    let config = generate_config(
        &agent_name,
        provider_name,
        &api_base,
        &model,
        &api_key,
        temperature,
        telegram_token.as_deref(),
        discord_token.as_deref(),
        github_token.as_deref(),
        &extra_mcp_servers,
    );
    
    fs::write(&config_path, &config)?;
    println!("\nConfiguration saved to {:?}", config_path);
    
    // Save token store
    if github_token.is_some() {
        let token_dir = config_dir.join("tokens.json");
        let mut tokens = serde_json::Map::new();
        if let Some(ref t) = github_token {
            tokens.insert("GITHUB_TOKEN".to_string(), serde_json::Value::String(t.clone()));
        }
        let token_json = serde_json::to_string_pretty(&tokens)?;
        fs::write(&token_dir, token_json)?;
        println!("Tokens saved to {:?}", token_dir);
    }
    
    // Summary
    println!("\nSummary:");
    println!("  Agent: {}", agent_name);
    println!("  Provider: {} ({})", provider_name, model);
    println!("  Temperature: {}", temperature);
    if enable_telegram {
        println!("  Telegram: enabled");
    }
    if enable_discord {
        println!("  Discord: enabled");
    }
    if enable_github_mcp {
        println!("  GitHub MCP: enabled (26 tools)");
    }
    if !extra_mcp_servers.is_empty() {
        println!("  Extra MCP servers: {}", extra_mcp_servers.len());
    }
    
    println!("\nSetup complete! Run `auxloclaw gateway` to start.");
    
    Ok(())
}

fn quick_setup(config_dir: &PathBuf, telegram: bool, discord: bool) -> Result<()> {
    if !config_dir.exists() {
        fs::create_dir_all(config_dir)?;
        fs::create_dir_all(config_dir.join("skills"))?;
        fs::create_dir_all(config_dir.join("memory"))?;
    }
    
    let config = generate_config(
        "AUXLOCLAW",
        "nvidia",
        "https://integrate.api.nvidia.com/v1",
        "stepfun-ai/step-3.5-flash",
        &std::env::var("NVIDIA_API_KEY").unwrap_or_default(),
        1.0,
        if telegram { Some("") } else { None },
        if discord { Some("") } else { None },
        None,
        &[],
    );
    
    let config_path = config_dir.join("config.toml");
    fs::write(&config_path, &config)?;
    
    println!("Quick setup complete: {:?}", config_path);
    println!("  Set your API key: export NVIDIA_API_KEY=your-key");
    println!("  Run: auxloclaw gateway");
    
    Ok(())
}

fn generate_config(
    agent_name: &str,
    provider: &str,
    api_base: &str,
    model: &str,
    api_key: &str,
    temperature: f32,
    telegram_token: Option<&str>,
    discord_token: Option<&str>,
    github_token: Option<&str>,
    extra_mcp: &[(String, String, Vec<String>)],
) -> String {
    let mut config = format!(r#"# AUXLOCLAW Configuration

[agent]
name = "{}"
default_model = "{}"
max_tokens = 8192
temperature = {}
max_tool_iterations = 100
context_window_tokens = 20000
timezone = "UTC"

[providers]
connection_pool_size = 32
request_timeout_secs = 120

[providers.primary]
name = "{}"
api_base = "{}"
api_key = "{}"

[providers.fallbacks]

[memory]
database_path = "~/.auxloclaw/memory.db"
hot_cache_size = 1000
session_max_messages = 100
consolidation_interval_secs = 300

[channels.telegram]
enabled = {}
token = "{}"
group_policy = "mention"

[channels.discord]
enabled = {}
token = "{}"
group_policy = "mention"
allowed_guilds = []

[channels.slack]
enabled = false
bot_token = ""
app_token = ""

[tools]
exec_enabled = true
exec_timeout_secs = 60
restrict_to_workspace = true
web_search_enabled = false
web_search_provider = "brave"

[server]
host = "0.0.0.0"
port = 18789
cors_enabled = true

[mcp]
enabled = true
"#,
        agent_name,
        model,
        temperature,
        provider,
        api_base,
        api_key,
        telegram_token.is_some() && telegram_token.unwrap_or("").trim().is_empty() == false,
        telegram_token.unwrap_or("").trim(),
        discord_token.is_some() && discord_token.unwrap_or("").trim().is_empty() == false,
        discord_token.unwrap_or("").trim()
    );

    // Add GitHub MCP server if token provided
    if github_token.is_some() {
        config.push_str(&format!(r#"
[[mcp.servers]]
name = "github"
command = "mcp-server-github"
args = []
tool_prefix = "github"
timeout_secs = 30

[mcp.servers.env]
GITHUB_PERSONAL_ACCESS_TOKEN = "{}"
"#, github_token.unwrap()));
    }

    // Add extra MCP servers
    for (name, command, args) in extra_mcp {
        let args_str: Vec<String> = args.iter().map(|a| format!("\"{}\"", a)).collect();
        config.push_str(&format!(r#"
[[mcp.servers]]
name = "{}"
command = "{}"
args = [{}]
tool_prefix = "{}"
timeout_secs = 30
"#, name, command, args_str.join(", "), name));
    }

    config
}
