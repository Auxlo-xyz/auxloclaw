//! Agent Core - Central orchestration
use anyhow::Result;
use std::sync::Arc;

use crate::config::AppConfig;
use crate::memory::{MemoryEngine, SessionHistory};
use crate::orchestrator::ToolOrchestrator;
use crate::providers::{CompletionRequest, Message, ProviderPool};
use crate::streaming::StreamSession;

/// Agent core
pub struct AgentCore {
    #[allow(dead_code)]
    config: AppConfig,
    memory: Arc<MemoryEngine>,
    providers: Arc<ProviderPool>,
    orchestrator: Arc<ToolOrchestrator>,
}

impl AgentCore {
    pub fn new(
        memory: Arc<MemoryEngine>,
        providers: Arc<ProviderPool>,
        orchestrator: Arc<ToolOrchestrator>,
        config: AppConfig,
    ) -> Self {
        Self {
            config,
            memory,
            providers,
            orchestrator,
        }
    }

    /// Process a message
    pub async fn process(&self, message: &str) -> String {
        let request = self.build_request(message).await;
        
        match self.providers.complete(request).await {
            Ok(response) => response.content,
            Err(e) => format!("Error: {}", e),
        }
    }

    /// Process with streaming
    pub async fn process_stream(&self, message: &str) -> StreamSession {
        let request = self.build_request(message).await;
        let _ = request; // Suppress unused warning
        
        let (session, _rx) = StreamSession::new(
            uuid(),
            100,
        );
        session
    }

    async fn build_request(&self, message: &str) -> CompletionRequest {
        CompletionRequest {
            model: self.config.agent.default_model.clone(),
            messages: vec![
                Message {
                    role: "system".into(),
                    content: "You are AUXLOCLAW, a high-performance AI agent.".into(),
                    tool_calls: None,
                },
                Message {
                    role: "user".into(),
                    content: message.to_string(),
                    tool_calls: None,
                },
            ],
            temperature: Some(self.config.agent.temperature),
            max_tokens: Some(self.config.agent.max_tokens),
            tools: None,
            stream: None,
        }
    }

    pub async fn remember(&self, key: &str, content: &str) -> Result<()> {
        self.memory.store(key, content, None).await
    }

    pub async fn recall(&self, key: &str) -> Option<String> {
        self.memory.retrieve(key).await.map(|e| e.content)
    }
}

fn uuid() -> String {
    format!("{:016x}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64)
}