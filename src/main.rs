//! AUXLOCLAW - Ultra-High-Performance AI Agent Framework

mod agent;
mod channels;
mod config;
mod persona;
mod memory;
mod providers;
mod skills;
mod tools;
mod streaming;
mod orchestrator;
mod cli;
mod commands;

use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use clap::Parser;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;
use tracing_subscriber::EnvFilter;

use cli::{Cli, Commands};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse_args();
    
    // Initialize logging
    let level = if args.debug { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(level))
        .init();
    
    // Handle commands
    match args.command {
        Commands::Gateway { port, host } => {
            run_gateway(&host, port).await?;
        }
        
        Commands::Chat { message, model, stream } => {
            commands::chat::handle_chat(message, model, stream).await?;
        }
        
        Commands::Setup { quick, telegram, discord } => {
            commands::setup::handle_setup(quick, telegram, discord)?;
        }
        
        Commands::Config { action } => {
            commands::config::handle_config(action)?;
        }
        
        Commands::Skill { action } => {
            commands::skill::handle_skill(action).await?;
        }
        
        Commands::Provider { action } => {
            commands::provider::handle_provider(action)?;
        }
        
        Commands::Persona { action } => {
            commands::persona::handle_persona(action)?;
        }
        
        Commands::Status => {
            commands::status::handle_status()?;
        }
        
        Commands::Run { skill, args: run_args } => {
            commands::run::handle_run(skill, run_args).await?;
        }
        
        Commands::Stop => {
            commands::stop::handle_stop()?;
        }
    }
    
    Ok(())
}

async fn run_gateway(host: &str, port: u16) -> anyhow::Result<()> {
    info!("🦞 AUXLOCLAW v0.1.0 initializing...");
    
    let start = Instant::now();
    
    // Load config
    let config_path = dirs::home_dir()
        .map(|h| h.join(".auxloclaw/config.toml"))
        .ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?;
    let config = config::AppConfig::load(config_path.to_str().unwrap_or("~/.auxloclaw/config.toml"))?;
    
    // Initialize core components
    let memory = Arc::new(memory::MemoryEngine::new(&config.memory).await?);
    let providers = Arc::new(providers::ProviderPool::new(config.providers.clone()));
    let orchestrator = Arc::new(orchestrator::ToolOrchestrator::new());
    let agent = Arc::new(agent::AgentCore::new(memory, providers, orchestrator, config.clone()));
    
    info!("⚡ Core initialized in {:?}", start.elapsed());
    
    // Start Telegram channel if enabled
    let tg_handle = if config.channels.telegram.enabled {
        let tg_agent = agent.clone();
        let tg_config = config.channels.telegram.clone();
        let tg_persona = config.persona.clone();
        info!("📱 Starting Telegram gateway...");
        Some(tokio::spawn(async move {
            if let Err(e) = channels::telegram::start(tg_agent, Some(tg_config), tg_persona).await {
                tracing::error!("Telegram error: {}", e);
            }
        }))
    } else {
        None
    };
    
    // Build HTTP router
    let app = Router::new()
        .route("/health", axum::routing::get(|| async { "OK" }))
        .route("/chat", axum::routing::post(chat_handler))
        .route("/api/chat", axum::routing::post(chat_handler))
        .route("/api/status", axum::routing::get(status_handler))
        .route("/api/skills", axum::routing::get(list_skills_handler))
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any))
        .layer(TraceLayer::new_for_http())
        .with_state(agent.clone());
    
    // Start server
    let addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    
    info!("🌐 HTTP server listening on {}", addr);
    info!("✅ Ready in {:?}", start.elapsed());
    
    axum::serve(listener, app).await?;
    
    Ok(())
}

use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
}

#[derive(Serialize)]
struct ChatResponse {
    response: String,
}

async fn chat_handler(
    axum::extract::State(agent): axum::extract::State<Arc<agent::AgentCore>>,
    axum::Json(req): axum::Json<ChatRequest>,
) -> axum::Json<ChatResponse> {
    let response = agent.process(&req.message, None).await;
    axum::Json(ChatResponse { response })
}

async fn status_handler() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "status": "ok",
        "version": "0.1.0",
        "uptime": "running"
    }))
}

async fn list_skills_handler() -> axum::Json<Vec<String>> {
    // Return list of available skills
    axum::Json(vec![
        "code-review".into(),
        "arxiv".into(),
        "fine-tuning-axolotl".into(),
    ])
}