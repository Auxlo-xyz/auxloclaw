//! Agent Core - Central orchestration

use anyhow::Result;
use std::sync::Arc;

use crate::config::AppConfig;
use crate::memory::{MemoryEngine, SessionHistory};
use crate::orchestrator::ToolOrchestrator;
use crate::providers::{CompletionRequest, Message, ProviderPool};
use crate::streaming::StreamSession;
use crate::persona::PersonaConfig;
use crate::skills::SkillRegistry;

/// Usage statistics
#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub total_messages: u64,
    pub total_tokens: u64,
}

/// Tool info
pub struct ToolInfo {
    pub name: String,
    pub description: String,
}

/// Agent core
pub struct AgentCore {
    config: AppConfig,
    memory: Arc<MemoryEngine>,
    providers: Arc<ProviderPool>,
    orchestrator: Arc<ToolOrchestrator>,
    persona: PersonaConfig,
    usage: std::sync::atomic::AtomicU64,
}

impl AgentCore {
    pub fn new(
        memory: Arc<MemoryEngine>,
        providers: Arc<ProviderPool>,
        orchestrator: Arc<ToolOrchestrator>,
        config: AppConfig,
    ) -> Self {
        let persona = config.persona.clone();
        Self {
            config,
            memory,
            providers,
            orchestrator,
            persona,
            usage: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Process a message
    pub async fn process(&self, message: &str) -> String {
        let request = self.build_request(message).await;
        
        match self.providers.complete(request).await {
            Ok(response) => {
                // Update usage
                if let Some(usage) = response.usage {
                    self.usage.fetch_add(usage.total_tokens as u64, std::sync::atomic::Ordering::Relaxed);
                }
                response.content
            },
            Err(e) => format!("Error: {}", e),
        }
    }

    /// Process with streaming
    pub async fn process_stream(&self, message: &str) -> StreamSession {
        let request = self.build_request(message).await;
        let _ = request;
        
        let (session, _rx) = StreamSession::new(
            uuid(),
            100,
        );
        session
    }

    async fn build_request(&self, message: &str) -> CompletionRequest {
        let system_prompt = self.build_system_prompt();
        
        // Convert orchestrator ToolDefinition to providers ToolDefinition
        let tools: Vec<crate::providers::ToolDefinition> = self.orchestrator.get_definitions()
            .into_iter()
            .map(|t| crate::providers::ToolDefinition {
                tool_type: t.tool_type,
                function: crate::providers::FunctionDefinition {
                    name: t.function.name,
                    description: t.function.description,
                    parameters: t.function.parameters,
                },
            })
            .collect();
        
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
            tools: Some(tools),
            stream: None,
        }
    }
    
    fn build_system_prompt(&self) -> String {
        use crate::persona::SystemPromptBuilder;
        
        let tools = self.orchestrator.get_definitions();
        let builder = SystemPromptBuilder::new(self.persona.clone())
            .with_tools(&tools)
            .with_skills(&[]); // TODO: Add skills
        
        builder.build()
    }

    // === Telegram command helpers ===
    
    /// Get memory summary
    pub async fn memory_summary(&self) -> String {
        // TODO: Get actual memory summary
        "No long-term memory stored yet.".into()
    }
    
    /// Clear session
    pub async fn clear_session(&self, _session_id: &str) -> Result<()> {
        // TODO: Implement session clearing
        Ok(())
    }
    
    /// Recover session
    pub async fn recover_session(&self, _session_id: &str) -> Result<()> {
        // TODO: Implement session recovery
        Ok(())
    }
    
    /// New session
    pub async fn new_session(&self, _session_id: &str) -> Result<()> {
        // TODO: Implement new session
        Ok(())
    }
    
    /// List tools
    pub fn list_tools(&self) -> Vec<ToolInfo> {
        self.orchestrator.get_definitions()
            .into_iter()
            .map(|t| ToolInfo {
                name: t.function.name,
                description: t.function.description,
            })
            .collect()
    }
    
    /// Get usage stats
    pub async fn get_usage_stats(&self) -> Usage {
        Usage {
            total_messages: 0,
            total_tokens: self.usage.load(std::sync::atomic::Ordering::Relaxed),
        }
    }
    
    /// Get model name
    pub fn model_name(&self) -> &str {
        &self.config.agent.default_model
    }
}

fn uuid() -> String {
    format!("{:016x}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64)
}