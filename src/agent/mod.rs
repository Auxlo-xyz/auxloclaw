//! Agent Core - Central orchestration

use anyhow::Result;
use std::sync::Arc;

use crate::config::AppConfig;
use crate::memory::{MemoryEngine, SessionHistory};
use crate::orchestrator::ToolOrchestrator;
use crate::providers::{CompletionRequest, Message, ProviderPool};
use crate::streaming::StreamSession;
use crate::persona::SystemPromptBuilder;

/// Agent core
pub struct AgentCore {
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

    /// Build the system prompt
    fn build_system_prompt(&self) -> String {
        // Get tool definitions
        let tools = self.orchestrator.get_definitions();
        
        // Get skills (placeholder for now)
        let skills: Vec<(String, String)> = vec![];
        
        // Build prompt using persona
        SystemPromptBuilder::new(self.config.persona.clone())
            .with_tools(&tools)
            .with_skills(&skills)
            .build()
    }

    async fn build_request(&self, message: &str) -> CompletionRequest {
        let system_prompt = self.build_system_prompt();
        
        CompletionRequest {
            model: self.config.agent.default_model.clone(),
            messages: vec![
                Message {
                    role: "system".into(),
                    content: system_prompt,
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