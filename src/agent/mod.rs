//! Agent Core - Central processing unit for the agent framework

use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

use crate::capabilities::CapabilityManifest;
use crate::checkpoints::CheckpointManager;
use crate::config::AppConfig;
use crate::context::build_pruned_messages;
use crate::memory::{
    CompactionResult, Compactor, HistoryMessage, MemoryEngine, Reflection, Reflector,
    ReflectorConfig, SessionHistory, SessionStore,
};
use crate::orchestrator::ToolOrchestrator;
use crate::persona::PersonaConfig;
use crate::plugins::{HookEvent, PluginManager};
use crate::providers::{CompletionRequest, Message, ProviderPool, ToolCall};
use regex::Regex;

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
    compactor: Arc<Compactor>,
    reflector: Arc<Reflector>,
    plugins: Arc<PluginManager>,
    checkpoint_manager: Arc<CheckpointManager>,
    /// Last activity timestamp per session (epoch seconds)
    last_activity: RwLock<HashMap<String, u64>>,
}

impl AgentCore {
    pub fn new(
        // This is a hack to avoid modifying the whole signature
        memory: Arc<MemoryEngine>,
        providers: Arc<ProviderPool>,
        orchestrator: Arc<ToolOrchestrator>,
        config: AppConfig,
        session_store: Arc<SessionStore>,
        plugins: Arc<PluginManager>,
        checkpoint_manager: Arc<CheckpointManager>,
    ) -> Result<Self> {
        let persona = config.persona.clone();
        let data_dir = PathBuf::from(shellexpand::tilde(&config.memory.database_path).into_owned())
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("~/.auxloclaw"));

        let compactor = Arc::new(Compactor::new(config.memory.clone(), data_dir.clone()));

        let reflector_config = ReflectorConfig {
            enabled: config.memory.reflection_enabled,
            min_messages: config.memory.reflection_min_messages,
            cooldown_secs: config.memory.reflection_cooldown_secs,
        };
        let reflector = Arc::new(Reflector::new(reflector_config, data_dir));

        Ok(Self {
            config,
            memory,
            providers,
            orchestrator,
            persona,
            usage: std::sync::atomic::AtomicU64::new(0),
            sessions: RwLock::new(HashMap::new()),
            session_store,
            compactor,
            reflector,
            plugins,
            checkpoint_manager,
            last_activity: RwLock::new(HashMap::new()),
        })
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

        let effective_message = self
            .plugins
            .process_message_hooks(
                HookEvent::BeforeMessage,
                Some(&session_key),
                message.to_string(),
            )
            .await;

        // Get history
        let history = self.get_history(&session_key).await;

        // Get reflections
        let reflections = self.get_reflections(&session_key).unwrap_or_else(Vec::new);

        // Build initial messages
        let mut messages = vec![Message {
            role: "system".into(),
            content: self.build_system_prompt(),
            tool_calls: None,
        }];

        // Add reflections as system messages
        for reflection in reflections {
            messages.push(Message {
                role: "system".into(),
                content: serde_json::to_string(&reflection)
                    .unwrap_or_else(|_| reflection.title.clone()),
                tool_calls: None,
            });
        }

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
            content: effective_message.clone(),
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
                        self.usage.fetch_add(
                            usage.total_tokens as u64,
                            std::sync::atomic::Ordering::Relaxed,
                        );
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
        self.add_to_history(&session_key, "user", &effective_message)
            .await;
        self.add_to_history(&session_key, "assistant", &final_response)
            .await;

        // Run compaction check after assistant response
        if let Some(result) = self.run_post_compaction_check(&session_key).await {
            tracing::info!(
                "Session compacted: {} messages saved ~{} tokens",
                result.compacted_messages,
                result.tokens_saved
            );
        }

        // Filter out <thought></thought> blocks including content using regex
        let re = Regex::new(r"(?s)<thought>.*?</thought>").unwrap();
        let filtered_response = re.replace_all(&final_response, "").to_string();
        let filtered_response = self
            .plugins
            .process_message_hooks(
                HookEvent::AfterMessage,
                Some(&session_key),
                filtered_response,
            )
            .await;

        // Update last activity timestamp for the session
        self.touch_activity(&session_key).await;

        filtered_response
    }

    async fn execute_tool(&self, tool_call: &ToolCall) -> String {
        let args: serde_json::Value =
            serde_json::from_str(&tool_call.function.arguments).unwrap_or(serde_json::Value::Null);

        tracing::info!(
            " Executing tool: {} with args: {}",
            tool_call.function.name,
            args
        );

        // Execute via orchestrator
        let result = self
            .orchestrator
            .execute_tool(&tool_call.function.name, args)
            .await;

        if result.success {
            serde_json::to_string(&result.output).unwrap_or_else(|_| result.output.to_string())
        } else {
            format!(
                "Tool error: {}",
                result.error.as_ref().unwrap_or(&"Unknown error".into())
            )
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

    pub async fn add_to_history(&self, session_key: &str, role: &str, content: &str) {
        let mut sessions = self.sessions.write().await;
        let session = sessions
            .entry(session_key.to_string())
            .or_insert_with(|| SessionHistory::new(session_key));

        session.add_message(role, content, None);

        // Note: removed the old 50 message limit - compaction handles this now

        let session_clone = session.clone();
        drop(sessions);

        let _ = self.session_store.save(session_key, &session_clone);
    }

    /// Run post-compaction check after assistant response
    /// Returns the compaction result if compaction was triggered
    pub async fn run_post_compaction_check(&self, session_key: &str) -> Option<CompactionResult> {
        // Get current message count
        let message_count = {
            let sessions = self.sessions.read().await;
            sessions
                .get(session_key)
                .map(|s| s.messages.len())
                .unwrap_or(0)
        };

        // Check if compaction should run
        if !self.compactor.should_compact(session_key, message_count) {
            return None;
        }

        tracing::info!(
            "Compaction triggered for session {} ({} messages >= threshold {})",
            session_key,
            message_count,
            self.config.memory.compaction_threshold
        );

        // Get mutable session and compact
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(session_key) {
            match self.compactor.compact(session).await {
                Ok(result) => {
                    if result.success {
                        tracing::info!(
                            "Compaction complete: {} -> {} messages, ~{} tokens saved",
                            result.original_messages,
                            result.compacted_messages,
                            result.tokens_saved
                        );

                        // Save compacted session
                        let session_clone = session.clone();
                        drop(sessions);
                        let _ = self.session_store.save(session_key, &session_clone);

                        return Some(result);
                    } else {
                        tracing::warn!(
                            "Compaction failed: {}",
                            result
                                .error
                                .as_ref()
                                .unwrap_or(&"Unknown error".to_string())
                        );
                    }
                }
                Err(e) => {
                    tracing::error!("Compaction error: {}", e);
                }
            }
        }

        None
    }

    fn build_request(&self, messages: Vec<Message>) -> CompletionRequest {
        let tools: Vec<crate::providers::ToolDefinition> = self
            .orchestrator
            .get_definitions()
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

    pub fn capability_manifest(&self) -> CapabilityManifest {
        CapabilityManifest::new(&self.config, Some(&self.orchestrator))
    }

    fn build_system_prompt(&self) -> String {
        use crate::persona::SystemPromptBuilder;

        let tools = self.orchestrator.get_definitions();
        let capability_summary = self.capability_manifest().prompt_summary();
        let base_prompt = SystemPromptBuilder::new(self.persona.clone())
            .with_tools(&tools)
            .with_skills(&[])
            .build();

        format!("{}\n\n{}", base_prompt, capability_summary)
    }

    pub async fn memory_summary(&self) -> String {
        let sessions = self.sessions.read().await;
        if sessions.is_empty() {
            return "No conversation history stored yet.".into();
        }

        let mut summary = String::new();
        for (key, session) in sessions.iter() {
            summary.push_str(&format!(
                "Session: {} ({} messages)\n",
                key,
                session.messages.len()
            ));
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
        self.orchestrator
            .get_definitions()
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

    /// Run reflection on a session
    /// Can be triggered explicitly or after significant activity
    pub async fn run_reflection(&self, session_key: &str) -> Option<Reflection> {
        // Get current message count
        let message_count = {
            let sessions = self.sessions.read().await;
            sessions
                .get(session_key)
                .map(|s| s.messages.len())
                .unwrap_or(0)
        };

        // Check if reflection should run
        if !self.reflector.should_reflect(session_key, message_count) {
            return None;
        }

        tracing::info!(
            "Reflection triggered for session {} ({} messages)",
            session_key,
            message_count
        );

        // Get session and reflect
        let sessions = self.sessions.read().await;
        if let Some(session) = sessions.get(session_key) {
            match self.reflector.reflect(session).await {
                Ok(reflection) => {
                    tracing::info!(
                        "Reflection complete: {} - {}",
                        reflection.reflection_type.to_string().to_lowercase(),
                        reflection.title
                    );
                    return Some(reflection);
                }
                Err(e) => {
                    tracing::error!("Reflection error: {}", e);
                }
            }
        }

        None
    }

    /// Update last activity timestamp for a session
    pub async fn touch_activity(&self, session_key: &str) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut last_activity = self.last_activity.write().await;
        last_activity.insert(session_key.to_string(), now);
    }

    /// Get sessions that have been inactive for longer than reflection_interval_secs
    /// and have enough messages to qualify for reflection
    pub async fn get_sessions_needing_reflection(&self) -> Vec<String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let interval_secs = self.config.memory.reflection_interval_secs;
        let min_messages = self.config.memory.reflection_min_messages;

        let sessions = self.sessions.read().await;
        let last_activity = self.last_activity.read().await;

        let mut result = Vec::new();

        for (key, session) in sessions.iter() {
            let last = last_activity.get(key).copied().unwrap_or(0);
            let inactive_secs = now.saturating_sub(last);

            // Check: inactive long enough AND has enough messages
            if inactive_secs >= interval_secs && session.messages.len() >= min_messages {
                result.push(key.clone());
            }
        }

        result
    }

    /// Get reflections for a session
    pub fn get_reflections(&self, session_key: &str) -> Option<Vec<Reflection>> {
        self.reflector.load_reflections(session_key).ok()
    }

    /// Get all reflections across all sessions
    pub fn get_all_reflections(&self) -> Option<Vec<Reflection>> {
        self.reflector.load_all_reflections().ok()
    }

    pub async fn create_checkpoint(&self, session_id: &str, label: Option<&str>) -> Result<String> {
        let sessions = self.sessions.read().await;
        if let Some(session) = sessions.get(session_id) {
            self.checkpoint_manager
                .create_checkpoint(session_id, session, label)
        } else {
            Err(anyhow::anyhow!("Session not found: {}", session_id))
        }
    }

    pub async fn rollback_session(&self, session_id: &str, checkpoint_id: &str) -> Result<()> {
        let history = self
            .checkpoint_manager
            .rollback(session_id, checkpoint_id)?;
        let mut sessions = self.sessions.write().await;
        sessions.insert(session_id.to_string(), history);
        Ok(())
    }
}
