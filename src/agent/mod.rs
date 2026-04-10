//! Agent Core - Central processing unit for the agent framework

use anyhow::Result;
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;

use crate::config::AppConfig;
use crate::memory::{MemoryEngine, SessionHistory, SessionStore, HistoryMessage};
use crate::orchestrator::ToolOrchestrator;
use crate::providers::{CompletionRequest, Message, ProviderPool, ToolCall};
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
    pub sessions: RwLock<HashMap<String, SessionHistory>>,
    session_store: Arc<SessionStore>,
}

impl AgentCore {
    pub fn new(
        memory: Arc<MemoryEngine>,
        providers: Arc<ProviderPool>,
        orchestrator: Arc<ToolOrchestrator>,
        config: AppConfig,
        session_store: Arc<SessionStore>,
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
            session_store,
        }
    }

    pub async fn load_sessions(&self) -> Result<()> {
        let persisted = self.session_store.load_all()?;
        let mut sessions = self.sessions.write().await;
        for (key, history) in persisted {
            sessions.insert(key, history);
        }
        tracing::info!(" Loaded {} sessions from disk", sessions.len());
        Ok(())
    }

    /// Process a message with tool execution loop
    pub async fn process(&self, message: &str, session_id: Option<&str>) -> String {
        let session_key = session_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "default".to_string());
        
        // Get history
        let history = self.get_history(&session_key).await;
        
        // Build initial messages
        let mut messages = vec![Message {
            role: "system".into(),
            content: self.build_system_prompt(),
            tool_calls: None,
        }];
        
        // Add history
        for m in history {
            messages.push(Message {
                role: m.role,
                content: m.content,
                tool_calls: None,
            });
        }
        
        // Add user message
        messages.push(Message {
            role: "user".into(),
            content: message.to_string(),
            tool_calls: None,
        });
        
        // Tool execution loop
        let mut iterations = 0;
        let max_iterations = self.config.agent.max_tool_iterations as usize;
        let mut final_response = String::new();
        
        loop {
            iterations += 1;
            if iterations > max_iterations {
                final_response = "Error: Max tool iterations reached".into();
                break;
            }
            
            let request = self.build_request(messages.clone());
            
            match self.providers.complete(request).await {
                Ok(response) => {
                    // Update usage
                    if let Some(usage) = &response.usage {
                        self.usage.fetch_add(usage.total_tokens as u64, std::sync::atomic::Ordering::Relaxed);
                    }
                    
                    // Check if there are tool calls
                    if let Some(tool_calls) = &response.tool_calls {
                        if !tool_calls.is_empty() {
                            tracing::debug!("Model returned {} tool_calls", tool_calls.len());
                            
                            // Add assistant message with tool calls
                            messages.push(Message {
                                role: "assistant".into(),
                                content: String::new(),
                                tool_calls: Some(tool_calls.clone()),
                            });
                            
                            // Execute each tool
                            for tool_call in tool_calls {
                                let result = self.execute_tool(tool_call).await;
                                
                                // Add tool result as a message
                                messages.push(Message {
                                    role: "tool".into(),
                                    content: result.clone(),
                                    tool_calls: None,
                                });
                            }
                            
                            tracing::debug!("Continuing loop with {} messages", messages.len());
                            // Continue loop to get next response
                            continue;
                        }
                    }
                    
                    // No tool calls - this is the final response
                    final_response = response.content;
                    break;
                }
                Err(e) => {
                    tracing::error!("Provider error: {}", e);
                    final_response = format!("Error: {}", e);
                    break;
                }
            }
        }
        
        // Store in history
        self.add_to_history(&session_key, "user", message).await;
        self.add_to_history(&session_key, "assistant", &final_response).await;
        
        // Filter out <thought></thought> tags
        let filtered_response = final_response.replace("<thought>", "").replace("</thought>", "");
        
        filtered_response
    }
    
    async fn execute_tool(&self, tool_call: &ToolCall) -> String {
        let args: serde_json::Value = serde_json::from_str(&tool_call.function.arguments)
            .unwrap_or(serde_json::Value::Null);
        
        tracing::info!(" Executing tool: {} with args: {}", tool_call.function.name, args);
        
        // Execute via orchestrator
        let result = self.orchestrator.execute_tool(&tool_call.function.name, args).await;
        
        if result.success {
            serde_json::to_string(&result.output).unwrap_or_else(|_| result.output.to_string())
        } else {
            format!("Tool error: {}", result.error.as_ref().unwrap_or(&"Unknown error".into()))
        }
    }
    
    async fn get_history(&self, session_key: &str) -> Vec<HistoryMessage> {
        let sessions = self.sessions.read().await;
        if let Some(session) = sessions.get(session_key) {
            session.messages.clone()
        } else {
            vec![]
        }
    }
    
    async fn add_to_history(&self, session_key: &str, role: &str, content: &str) {
        let mut sessions = self.sessions.write().await;
        let session = sessions
            .entry(session_key.to_string())
            .or_insert_with(|| SessionHistory::new(session_key));
        
        session.add_message(role, content, None);
        
        if session.messages.len() > 50 {
            let remove_count = session.messages.len() - 50;
            session.messages.drain(0..remove_count);
        }
        
        let session_clone = session.clone();
        drop(sessions);
        
        let _ = self.session_store.save(session_key, &session_clone);
    }
    
    fn build_request(&self, messages: Vec<Message>) -> CompletionRequest {
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
        SystemPromptBuilder::new(self.persona.clone())
            .with_tools(&tools)
            .with_skills(&[])
            .build()
    }

    pub async fn memory_summary(&self) -> String {
        let sessions = self.sessions.read().await;
        if sessions.is_empty() {
            return "No conversation history stored yet.".into();
        }
        
        let mut summary = String::new();
        for (key, session) in sessions.iter() {
            summary.push_str(&format!("Session: {} ({} messages)\n", key, session.messages.len()));
        }
        summary
    }
    
    /// Clear a session from memory and disk
    pub async fn clear_session(&self, session_id: &str) {
        // Remove from in-memory sessions
        self.sessions.write().await.remove(session_id);
        
        // Delete from disk
        if let Err(e) = self.session_store.delete(session_id) {
            tracing::warn!("Failed to delete session file: {}", e);
        }
        
        tracing::info!("Cleared session: {}", session_id);
    }
    
    pub async fn recover_session(&self, _session_id: &str) -> Result<()> {
        Ok(())
    }
    
    pub async fn new_session(&self, session_id: &str) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        sessions.insert(session_id.to_string(), SessionHistory::new(session_id));
        let session = sessions.get(session_id).cloned().unwrap();
        drop(sessions);
        self.session_store.save(session_id, &session)?;
        Ok(())
    }
    
    pub fn list_tools(&self) -> Vec<ToolInfo> {
        self.orchestrator.get_definitions()
            .into_iter()
            .map(|t| ToolInfo {
                name: t.function.name,
                description: t.function.description,
            })
            .collect()
    }
    
    pub async fn get_usage_stats(&self) -> Usage {
        Usage {
            total_messages: 0,
            total_tokens: self.usage.load(std::sync::atomic::Ordering::Relaxed),
        }
    }
    
    pub fn model_name(&self) -> &str {
        &self.config.agent.default_model
    }
}
