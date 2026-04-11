//! Structured Error Recovery Paths
//! Provides typed errors with recovery strategies

use anyhow::{anyhow, Result};
use std::time::Duration;
use tokio::time::sleep;

/// Agent error categories with recovery hints
#[derive(Debug, Clone)]
pub enum AgentError {
    /// Provider failed - try fallback or retry
    ProviderError {
        provider: String,
        message: String,
        retryable: bool,
        suggested_action: RecoveryAction,
    },
    /// Tool execution failed
    ToolError {
        tool: String,
        message: String,
        context: String,
        suggested_action: RecoveryAction,
    },
    /// Session/context error
    SessionError {
        session_id: String,
        message: String,
        suggested_action: RecoveryAction,
    },
    /// Rate limit hit
    RateLimitError {
        provider: String,
        retry_after_secs: u64,
    },
    /// Context too long
    ContextOverflow {
        tokens: u64,
        limit: u64,
        suggested_action: RecoveryAction,
    },
    /// Sub-agent failure
    SubAgentError {
        agent_id: String,
        task: String,
        error: String,
    },
    /// Timeout exceeded
    TimeoutError {
        operation: String,
        duration_secs: u64,
    },
}

/// Recovery actions the system can take
#[derive(Debug, Clone)]
pub enum RecoveryAction {
    /// Retry with exponential backoff
    RetryWithBackoff { max_attempts: u32, base_delay_ms: u64 },
    /// Switch to fallback provider
    SwitchToFallback { fallback_name: String },
    /// Truncate context and retry
    TruncateContext { keep_last_n: usize },
    /// Graceful degradation - return partial result
    DegradedResponse { message: String },
    /// Notify user and wait for input
    RequestUserInput { prompt: String },
    /// No automatic recovery possible
    NoRecovery { reason: String },
    /// Restart the session
    RestartSession,
    /// Spawn sub-agent to handle
    DelegateToSubAgent { agent_type: String },
}


impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.user_message())
    }
}

impl std::error::Error for AgentError {}

impl AgentError {
    /// Convert to user-friendly message
    pub fn user_message(&self) -> String {
        match self {
            Self::ProviderError { provider, message, .. } => {
                format!("Connection issue with {}. Retrying...", provider)
            }
            Self::ToolError { tool, .. } => {
                format!("Tool '{}' encountered an issue. Trying alternative approach...", tool)
            }
            Self::SessionError { .. } => {
                "Session issue detected. Starting fresh...".to_string()
            }
            Self::RateLimitError { retry_after_secs, .. } => {
                format!("Rate limit reached. Waiting {} seconds...", retry_after_secs)
            }
            Self::ContextOverflow { .. } => {
                "Context is getting long. Summarizing earlier conversation...".to_string()
            }
            Self::SubAgentError { agent_id, .. } => {
                format!("Sub-agent {} encountered an issue. Retrying...", agent_id)
            }
            Self::TimeoutError { operation, .. } => {
                format!("{} is taking longer than expected. Please wait...", operation)
            }
        }
    }

    /// Check if error is recoverable
    pub fn is_recoverable(&self) -> bool {
        !matches!(
            self.suggested_action(),
            RecoveryAction::NoRecovery { .. }
        )
    }

    /// Get suggested recovery action
    pub fn suggested_action(&self) -> RecoveryAction {
        match self {
            Self::ProviderError { suggested_action, .. } => suggested_action.clone(),
            Self::ToolError { suggested_action, .. } => suggested_action.clone(),
            Self::SessionError { suggested_action, .. } => suggested_action.clone(),
            Self::RateLimitError { retry_after_secs, .. } => {
                RecoveryAction::RetryWithBackoff {
                    max_attempts: 1,
                    base_delay_ms: retry_after_secs * 1000,
                }
            }
            Self::ContextOverflow { .. } => {
                RecoveryAction::TruncateContext { keep_last_n: 20 }
            }
            Self::SubAgentError { .. } => {
                RecoveryAction::RetryWithBackoff {
                    max_attempts: 2,
                    base_delay_ms: 1000,
                }
            }
            Self::TimeoutError { .. } => {
                RecoveryAction::RetryWithBackoff {
                    max_attempts: 1,
                    base_delay_ms: 2000,
                }
            }
        }
    }
}

/// Error recovery executor
pub struct ErrorRecovery {
    max_retries: u32,
}

impl ErrorRecovery {
    pub fn new(max_retries: u32) -> Self {
        Self { max_retries }
    }

    /// Execute recovery action and return success/failure
    pub async fn execute_recovery(&self, action: &RecoveryAction) -> Result<()> {
        match action {
            RecoveryAction::RetryWithBackoff { max_attempts, base_delay_ms } => {
                let attempts = max_attempts.min(&self.max_retries);
                for attempt in 0..*attempts {
                    let delay = base_delay_ms * 2u64.pow(attempt);
                    sleep(Duration::from_millis(delay)).await;
                    tracing::info!("Retry attempt {} after {}ms", attempt + 1, delay);
                }
                Ok(())
            }
            RecoveryAction::TruncateContext { keep_last_n } => {
                tracing::info!("Truncating context to last {} messages", keep_last_n);
                Ok(())
            }
            RecoveryAction::RestartSession => {
                tracing::info!("Restarting session");
                Ok(())
            }
            RecoveryAction::DegradedResponse { message } => {
                tracing::warn!("Degraded response: {}", message);
                Ok(())
            }
            RecoveryAction::NoRecovery { reason } => {
                Err(anyhow!("No recovery possible: {}", reason))
            }
            RecoveryAction::SwitchToFallback { fallback_name } => {
                tracing::info!("Switching to fallback provider: {}", fallback_name);
                Ok(())
            }
            RecoveryAction::RequestUserInput { prompt } => {
                tracing::info!("Requesting user input: {}", prompt);
                Ok(())
            }
            RecoveryAction::DelegateToSubAgent { agent_type } => {
                tracing::info!("Delegating to sub-agent: {}", agent_type);
                Ok(())
            }
        }
    }

    /// Handle error with recovery
    pub async fn handle_error(&self, error: AgentError) -> Result<()> {
        if error.is_recoverable() {
            let action = error.suggested_action();
            self.execute_recovery(&action).await
        } else {
            Err(anyhow!("{}", error.user_message()))
        }
    }
}

/// Result with error context
#[derive(Debug)]
pub struct AgentResult<T> {
    pub value: Option<T>,
    pub error: Option<AgentError>,
    pub context: String,
    pub partial_success: bool,
}

impl<T> AgentResult<T> {
    pub fn success(value: T, context: String) -> Self {
        Self {
            value: Some(value),
            error: None,
            context,
            partial_success: false,
        }
    }

    pub fn failure(error: AgentError, context: String) -> Self {
        Self {
            value: None,
            error: Some(error),
            context,
            partial_success: false,
        }
    }

    pub fn partial(value: T, error: AgentError, context: String) -> Self {
        Self {
            value: Some(value),
            error: Some(error),
            context,
            partial_success: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_error_recovery() {
        let error = AgentError::ProviderError {
            provider: "nvidia".to_string(),
            message: "Connection timeout".to_string(),
            retryable: true,
            suggested_action: RecoveryAction::RetryWithBackoff {
                max_attempts: 3,
                base_delay_ms: 1000,
            },
        };
        
        assert!(error.is_recoverable());
        assert!(error.user_message().contains("nvidia"));
    }
}
