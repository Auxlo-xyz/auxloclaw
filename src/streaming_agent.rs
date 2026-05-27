//! Streaming Agent - Async generator pattern for real-time tool execution
//! Based on Claude Code's architecture

use crate::agent::build_nudge_message;
use anyhow::Result;
use regex::Regex;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::RwLock;

use crate::config::AppConfig;
use crate::context::build_pruned_messages;
use crate::memory::{HistoryMessage, SessionHistory, SessionStore};
use crate::orchestrator::ToolOrchestrator;
use crate::persona::SystemPromptBuilder;
use crate::persona::{shared::load_current_persona, PersonaConfig};
use crate::providers::{CompletionRequest, Message, ProviderPool, StreamChunk, ToolCall};
use crate::agent::{Intervention, InterventionRegistry};

/// Agent events streamed to consumers
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// Text chunk from the model
    TextChunk(String),
    /// Model started generating a tool call
    ToolCallStart {
        id: String,
        name: String,
        arguments: String,
    },
    /// Tool execution completed
    ToolCallResult {
        id: String,
        name: String,
        result: String,
    },
    /// Tool execution failed  
    ToolCallError {
        id: String,
        name: String,
        error: String,
    },
    /// All tool calls in a round completed
    ToolRoundComplete,
    /// Final response ready
    Done { response: String },
    /// Error occurred
    Error(String),
}

/// Check if a tool is read-only (can run in parallel)
fn is_readonly_tool(name: &str) -> bool {
    matches!(
        name,
        "grep"
            | "glob"
            | "read_file"
            | "file_search"
            | "search"
            | "query"
            | "get"
            | "list"
            | "view"
            | "browser_navigate"
            | "read_webpage"
            | "view_webpage"
    )
}

/// Streaming agent wrapper
pub struct StreamingAgent {
    config: AppConfig,
    providers: Arc<ProviderPool>,
    orchestrator: Arc<ToolOrchestrator>,
    persona: PersonaConfig,
    sessions: Arc<RwLock<HashMap<String, SessionHistory>>>,
    session_store: Arc<SessionStore>,
    intervention_registry: Arc<InterventionRegistry>,
}

impl StreamingAgent {
    pub fn new(
        config: AppConfig,
        providers: Arc<ProviderPool>,
        orchestrator: Arc<ToolOrchestrator>,
        session_store: Arc<SessionStore>,
        intervention_registry: Arc<InterventionRegistry>,
    ) -> Self {
        let persona = load_current_persona().unwrap_or_else(|err| {
            tracing::warn!(
                "Failed to load current persona, using config persona: {}",
                err
            );
            config.persona.clone()
        });
        Self {
            config,
            providers,
            orchestrator,
            persona,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            session_store,
            intervention_registry,
        }
    }

    /// Load persisted sessions
    pub async fn load_sessions(&self) -> Result<()> {
        let persisted = self.session_store.load_all()?;
        let mut sessions = self.sessions.write().await;
        for (key, history) in persisted {
            sessions.insert(key, history);
        }
        tracing::info!(" Loaded {} sessions from disk", sessions.len());
        Ok(())
    }

    /// Process message with streaming - returns mpsc channel of events
    pub async fn process_streaming(
        &self,
        message: &str,
        session_id: Option<&str>,
    ) -> mpsc::Receiver<AgentEvent> {
        let (tx, rx) = mpsc::channel(100);

        let session_key = session_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "default".to_string());

        let config = self.config.clone();
        let orchestrator = self.orchestrator.clone();
        let providers = self.providers.clone();
        let sessions = self.sessions.clone();
        let session_store = self.session_store.clone();
        let persona = load_current_persona().unwrap_or_else(|err| {
            tracing::warn!(
                "Failed to reload current persona, using cached persona: {}",
                err
            );
            self.persona.clone()
        });
        let message = message.to_string(); // Clone for 'static lifetime
        let intervention_registry = self.intervention_registry.clone();

        tokio::spawn(async move {
            // Build system prompt
            let tools = orchestrator.get_definitions();
            let system_prompt = SystemPromptBuilder::new(persona.clone())
                .with_tools(&tools)
                .with_skills(&[])
                .build();

            // Get history
            let history = {
                let sessions_guard = sessions.read().await;
                sessions_guard
                    .get(&session_key)
                    .map(|s| s.messages.clone())
                    .unwrap_or_default()
            };

            // Build messages
            let mut messages = build_pruned_messages(
                system_prompt,
                &history,
                message.clone(),
                config.agent.recent_history_turns,
                config.agent.context_window_tokens,
            );

            let mut iterations = 0;
            let max_iterations = config.agent.max_tool_iterations as usize;
            let nudge_threshold = config.agent.nudge_after_tool_calls;
            let mut total_tool_calls: u32 = 0;
            let mut accumulated_text = String::new();

            // Register intervention channel for this session
            let mut intervention_rx = intervention_registry.register(&session_key).await;

            loop {
                iterations += 1;
                // Drain any mid-loop interventions
                while let Ok(intervention) = intervention_rx.try_recv() {
                    messages.push(Message::new("user", &intervention.message));
                    tracing::debug!("Intervention injected into streaming session {}", session_key);
                }
                if iterations > max_iterations {
                    let _ = tx
                        .send(AgentEvent::Error("Max tool iterations reached".into()))
                        .await;
                    break;
                }

                // Build tools list
                let tools: Vec<_> = orchestrator
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

                let request = CompletionRequest {
                    model: config.agent.default_model.clone(),
                    messages: messages.clone(),
                    temperature: Some(config.agent.temperature),
                    max_tokens: Some(config.agent.max_tokens),
                    tools: Some(tools),
                    stream: Some(true),
                    base_url: None,
                    api_key: None,
                };

                // Get streaming response
                let mut stream_rx = match providers.stream(request).await {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = tx
                            .send(AgentEvent::Error(format!("Provider error: {}", e)))
                            .await;
                        break;
                    }
                };

                accumulated_text.clear();
                let mut pending_tool_calls: Vec<ToolCall> = Vec::new();

                // Process stream
                while let Some(chunk) = stream_rx.recv().await {
                    for choice in chunk.choices {
                        let delta = choice.delta;

                        if !delta.content.is_empty() {
                            accumulated_text.push_str(&delta.content);
                            let _ = tx.send(AgentEvent::TextChunk(delta.content.clone())).await;
                        }

                        if let Some(tool_calls) = delta.tool_calls {
                            for tc in tool_calls {
                                let id = tc.id.clone();
                                let name = tc.function.name.clone();
                                let args = tc.function.arguments.clone();

                                let _ = tx
                                    .send(AgentEvent::ToolCallStart {
                                        id: id.clone(),
                                        name: name.clone(),
                                        arguments: args.clone(),
                                    })
                                    .await;

                                pending_tool_calls.push(tc);
                            }
                        }
                    }

                    if chunk.done {
                        break;
                    }
                }

                if !pending_tool_calls.is_empty() {
                    // Add assistant message with tool calls
                    messages.push(Message::with_tool_calls("assistant", pending_tool_calls.clone()));

                    // Classify tools
                    let mut read_only: Vec<ToolCall> = Vec::new();
                    let mut write_tools: Vec<ToolCall> = Vec::new();

                    for tc in &pending_tool_calls {
                        if is_readonly_tool(&tc.function.name) {
                            read_only.push(tc.clone());
                        } else {
                            write_tools.push(tc.clone());
                        }
                    }

                    // Execute read-only in parallel
                    let mut handles = Vec::new();
                    for tc in read_only {
                        let orch = orchestrator.clone();
                        let tc_id = tc.id.clone();
                        let tc_name = tc.function.name.clone();
                        let tc_args = tc.function.arguments.clone();
                        handles.push(tokio::spawn(async move {
                            let args: serde_json::Value =
                                serde_json::from_str(&tc_args).unwrap_or(serde_json::Value::Null);
                            let result = orch.execute_tool(&tc_name, args).await;
                            (tc_id, tc_name, result)
                        }));
                    }

                    // Collect parallel results
                    for handle in handles {
                        if let Ok((id, name, result)) = handle.await {
                            let result_str = if result.success {
                                serde_json::to_string(&result.output)
                                    .unwrap_or_else(|_| result.output.to_string())
                            } else {
                                format!(
                                    "Tool error: {}",
                                    result.error.unwrap_or_else(|| "Unknown error".into())
                                )
                            };

                            let _ = tx
                                .send(AgentEvent::ToolCallResult {
                                    id: id.clone(),
                                    name: name.clone(),
                                    result: result_str.clone(),
                                })
                                .await;

                            messages.push(Message::tool_result(id.clone(), name.clone(), crate::context::spill_tool_output(&result_str, config.agent.tool_output_max_chars, &name)));
                        }
                    }

                    // Execute write tools serially
                    for tc in write_tools {
                        let args: serde_json::Value = serde_json::from_str(&tc.function.arguments)
                            .unwrap_or(serde_json::Value::Null);

                        let result = orchestrator.execute_tool(&tc.function.name, args).await;

                        let result_str = if result.success {
                            serde_json::to_string(&result.output)
                                .unwrap_or_else(|_| result.output.to_string())
                        } else {
                            let error_msg = result.error.unwrap_or_else(|| "Unknown error".into());

                            let _ = tx
                                .send(AgentEvent::ToolCallError {
                                    id: tc.id.clone(),
                                    name: tc.function.name.clone(),
                                    error: error_msg.clone(),
                                })
                                .await;

                            error_msg
                        };

                        if result.success {
                            let _ = tx
                                .send(AgentEvent::ToolCallResult {
                                    id: tc.id.clone(),
                                    name: tc.function.name.clone(),
                                    result: result_str.clone(),
                                })
                                .await;
                        }

                        messages.push(Message::tool_result(tc.id.clone(), tc.function.name.clone(), crate::context::spill_tool_output(&result_str, config.agent.tool_output_max_chars, &tc.function.name)));
                    }

                    let _ = tx.send(AgentEvent::ToolRoundComplete).await;

                    // Count tool calls and check nudge threshold
                    total_tool_calls += pending_tool_calls.len() as u32;
                    if nudge_threshold > 0 && total_tool_calls >= nudge_threshold {
                        let nudge = build_nudge_message(total_tool_calls);
                        messages.push(Message::new("user", &nudge));
                        total_tool_calls = 0;
                        tracing::info!("Nudge injected after {} tool calls (streaming)", nudge_threshold);
                    }

                    continue;
                }

                // No tool calls - final response
                let re = Regex::new(r"(?s)<thought>.*?</thought>").unwrap();
                let filtered = re.replace_all(&accumulated_text, "").to_string();

                // Store in history
                {
                    let mut sessions_guard = sessions.write().await;
                    let session = sessions_guard
                        .entry(session_key.clone())
                        .or_insert_with(|| SessionHistory::new(&session_key));

                    session.add_message("user", &message, None);
                    session.add_message("assistant", &filtered, None);

                    if session.messages.len() > 50 {
                        let remove_count = session.messages.len() - 50;
                        session.messages.drain(0..remove_count);
                    }

                    let session_clone = session.clone();
                    drop(sessions_guard);

                    let _ = session_store.save(&session_key, &session_clone);
                }

                let _ = tx.send(AgentEvent::Done { response: filtered }).await;
                break;
            }
            // Unregister intervention channel
            intervention_registry.unregister(&session_key).await;
        });

        rx
    }
}
