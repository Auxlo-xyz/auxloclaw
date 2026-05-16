//! AUXLOCLAW - Ultra-High-Performance AI Agent Framework

mod agent;
mod auth;
mod channels;
mod cli;
mod commands;
mod config;
mod coordination;
mod error_recovery;
mod mcp;
mod memory;
mod orchestrator;
mod persona;
mod providers;
mod skills;
mod streaming;
mod tools;

use std::sync::Arc;
use std::time::Instant;

use crate::auth::{AuthConfig, AuthState};
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;
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

        Commands::Chat {
            message,
            model,
            stream,
        } => {
            commands::chat::handle_chat(message, model, stream).await?;
        }

        Commands::Setup {
            quick,
            telegram,
            discord,
        } => {
            commands::setup::handle_setup(quick, telegram, discord)?;
        }

        Commands::Config { action } => {
            commands::config::handle_config(action)?;
        }

        Commands::Skill { action } => {
            commands::skill::handle_skill(action).await?;
        }

        Commands::Provider { action } => {
            commands::provider::handle_provider(action).await?;
        }

        Commands::Persona { action } => {
            commands::persona::handle_persona(action)?;
        }

        Commands::Status { delegation } => {
            commands::status::handle_status(delegation)?;
        }

        Commands::Run {
            skill,
            args: run_args,
        } => {
            commands::run::handle_run(skill, run_args).await?;
        }

        Commands::Stop => {
            commands::stop::handle_stop()?;
        }
    }

    Ok(())
}

#[derive(Clone)]
struct AppState {
    agent: Arc<agent::AgentCore>,
}

async fn run_gateway(host: &str, port: u16) -> anyhow::Result<()> {
    let auth_config = AuthConfig {
        api_key: std::env::var("AUXLOCLAW_API_KEY").ok(),
        require_auth: std::env::var("AUXLOCLAW_REQUIRE_AUTH")
            .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(false),
    };
    let auth_state = Arc::new(AuthState::new(auth_config));

    info!("🦞 AUXLOCLAW v0.1.0 initializing...");

    let start = Instant::now();

    // Load config
    let config_path = dirs::home_dir()
        .map(|h| h.join(".auxloclaw/config.toml"))
        .ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?;
    let config =
        config::AppConfig::load(config_path.to_str().unwrap_or("~/.auxloclaw/config.toml"))?;

    // Expand tilde in database path
    let session_db = shellexpand::tilde(&config.memory.database_path).into_owned();

    // Initialize core components
    let memory = Arc::new(memory::MemoryEngine::new(&config.memory)?);
    let providers = Arc::new(providers::ProviderPool::new(config.providers.clone()));
    let orchestrator = Arc::new(orchestrator::ToolOrchestrator::new());
    if config.mcp.enabled {
        let count = orchestrator.register_mcp_tools(&config.mcp).await?;
        info!("🔌 Registered {} MCP tools", count);
    }

    // Initialize persistent session store
    let session_store = Arc::new(memory::SessionStore::new(&session_db)?);

    let agent = Arc::new(agent::AgentCore::new(
        memory,
        providers,
        orchestrator,
        config.clone(),
        session_store,
    ));

    // Load persisted sessions
    agent.load_sessions().await?;

    info!("⚡ Core initialized in {:?}", start.elapsed());

    // Spawn reflection monitor background task
    let monitor_agent = agent.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;

            // Check for sessions needing reflection
            let sessions = monitor_agent.get_sessions_needing_reflection().await;

            for session_key in sessions {
                tracing::info!(
                    "Auto-reflection triggered for inactive session: {}",
                    session_key
                );
                if let Some(reflection) = monitor_agent.run_reflection(&session_key).await {
                    tracing::info!(
                        "Auto-reflection complete: {} - {}",
                        reflection.reflection_type.to_string().to_lowercase(),
                        reflection.title
                    );
                }
            }
        }
    });

    // Start Telegram channel if enabled
    let _discord_handle = if config.channels.discord.enabled {
        let discord_agent = agent.clone();
        let discord_config = config.channels.discord.clone();
        info!("💬 Starting Discord gateway...");
        Some(tokio::spawn(async move {
            if let Err(e) = channels::discord::start(discord_agent, Some(discord_config)).await {
                tracing::error!("Discord error: {}", e);
            }
        }))
    } else {
        None
    };

    let _tg_handle = if config.channels.telegram.enabled {
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

    let state = AppState {
        agent: agent.clone(),
    };

    let auth_middleware_state = auth_state.clone();
    let auth_middleware = axum::middleware::from_fn(move |req: Request, next: Next| {
        let auth_state = auth_middleware_state.clone();
        async move {
            if let Err(e) = auth_state.verify_bearer_token(
                req.headers()
                    .get("authorization")
                    .and_then(|h| h.to_str().ok()),
            ) {
                return Response::builder()
                    .status(StatusCode::UNAUTHORIZED)
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::json!({"error": "Unauthorized", "message": e.to_string()})
                            .to_string(),
                    ))
                    .unwrap();
            }
            next.run(req).await
        }
    });

    // Build HTTP router
    let app = Router::new()
        .route("/health", axum::routing::get(|| async { "OK" }))
        .route("/chat", axum::routing::post(chat_handler))
        .route("/api/chat", axum::routing::post(chat_handler))
        .route("/api/status", axum::routing::get(status_handler))
        .route("/api/skills", axum::routing::get(list_skills_handler))
        .route("/api/reflect", axum::routing::post(reflect_handler))
        .route(
            "/api/reflections",
            axum::routing::get(list_reflections_handler),
        )
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(auth_middleware)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

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
    State(state): State<AppState>,
    axum::Json(req): axum::Json<ChatRequest>,
) -> axum::Json<ChatResponse> {
    let agent = state.agent;
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
    axum::Json(vec![
        "code-review".into(),
        "arxiv".into(),
        "fine-tuning-axolotl".into(),
    ])
}

#[derive(Deserialize)]
struct ReflectRequest {
    session_id: Option<String>,
}

async fn reflect_handler(
    State(state): State<AppState>,
    axum::Json(req): axum::Json<ReflectRequest>,
) -> axum::Json<serde_json::Value> {
    let agent = state.agent;
    let session_key = req.session_id.unwrap_or_else(|| "default".to_string());

    match agent.run_reflection(&session_key).await {
        Some(reflection) => axum::Json(
            serde_json::to_value(&reflection)
                .unwrap_or_else(|_| serde_json::json!({"error": "Failed to serialize reflection"})),
        ),
        None => axum::Json(serde_json::json!({
            "error": "Reflection not triggered (check min_messages or cooldown)"
        })),
    }
}

async fn list_reflections_handler(
    State(state): State<AppState>,
) -> axum::Json<Vec<serde_json::Value>> {
    let agent = state.agent;
    match agent.get_all_reflections() {
        Some(reflections) => axum::Json(
            reflections
                .iter()
                .map(|r| serde_json::to_value(r).unwrap_or_default())
                .collect(),
        ),
        None => axum::Json(vec![]),
    }
}

mod streaming_agent;
