//! Agent Core - Central orchestration

use anyhow::Result;
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;

use crate::config::AppConfig;
use crate::memory::{MemoryEngine, SessionHistory};
use crate::orchestrator::ToolOrchestrator;
use crate::providers::{CompletionRequest, Message, ProviderPool};
use crate::streaming::StreamSession;
use crate::persona::PersonaConfig;

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
    /// Session histories per chat (50 message rolling window)
    sessions: RwLock<HashMap<String, SessionHistory>>,
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
            sessions: RwLock::new(HashMap::new()),
        }
    }

    /// Process a message with optional session ID for history tracking
    pub async fn process(&self, message: &str, session_id: Option<i64>) -> String {
        let session_key = session_id
            .map(|id| format!("tg:{}", id))
            .unwrap_or_else(|| "default".to_string());
        
        // Build request with history
        let request = self.build_request_with_history(message, &session_key).await;
        
        match self.providers.complete(request).await {
            Ok(response) => {
                // Store in session history
                self.add_to_history(&session_key, "user", message).await;
                self.add_to_history(&session_key, "assistant", &response.content).await;
                
                // Update usage
                if let Some(usage) = response.usage {
                    self.usage.fetch_add(usage.total_tokens as u64, std::sync::atomic::Ordering::Relaxed);
                }
                response.content
            },
            Err(e) => format!("Error: {}", e),
        }
    }

    async fn add_to_history(&self, session_key: &str, role: &str, content: &str) {
        let mut sessions = self.sessions.write().await;
        let session = sessions
            .entry(session_key.to_string())
            .or_insert_with(|| SessionHistory::new(session_key));
        
        session.add_message(role, content, None);
        
        // Keep only last 50 messages (rolling window)
        if session.messages.len() > 50 {
            let remove_count = session.messages.len() - 50;
            session.messages.drain(0..remove_count);
        }
    }

    async fn build_request_with_history(&self, message: &str, session_key: &str) -> CompletionRequest {
        let system_prompt = self.build_system_prompt();
        
        // Get session history
        let sessions = self.sessions.read().await;
        let history_messages = if let Some(session) = sessions.get(session_key) {
            session.messages.iter().map(|m| Message {
                role: m.role.clone(),
                content: m.content.clone(),
                tool_calls: None,
            }).collect()
        } else {
            vec![]
        };
        drop(sessions);
        
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
        
        // Build messages: system + history + current
        let mut messages = vec![
            Message {
                role: "system".into(),
                content: system_prompt,
                tool_calls: None,
            },
        ];
        messages.extend(history_messages);
        messages.push(Message {
            role: "user".into(),
            content: message.to_string(),
            tool_calls: None,
        });
        
        CompletionRequest {
            model: self.config.agent.default_model.clone(),
            messages,
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
            .with_skills(&[]);
        
        builder.build()
    }

    // === Telegram command helpers ===
    
    /// Get memory summary
    pub async fn memory_summary(&self) -> String {
        let sessions = self.sessions.read().await;
        if sessions.is_empty() {
            return "No conversation history stored yet.".to_string();
        }
        
        let mut summary = String::new();
        for (key, session) in sessions.iter() {
            summary.push_str(&format!("Session: {} ({} messages)\n", key, session.messages.len()));
        }
        summary
    }
    
    /// Clear session
    pub async fn clear_session(&self, session_id: &str) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        sessions.remove(session_id);
        Ok(())
    }
    
    /// Recover session
    pub async fn recover_session(&self, _session_id: &str) -> Result<()> {
        Ok(())
    }
    
    /// New session
    pub async fn new_session(&self, session_id: &str) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        sessions.insert(session_id.to_string(), SessionHistory::new(session_id));
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
