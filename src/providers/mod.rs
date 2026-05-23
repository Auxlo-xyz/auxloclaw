//! Provider pool with multi-provider support
//! Users can choose which provider/model to use at any time

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use dashmap::DashMap;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, RwLock};
use tokio::time;

use crate::config::{ProviderEntry, ProvidersConfig};
use sha2::{Digest, Sha256};

/// LLM Provider trait - unified interface for all providers
#[async_trait]
pub trait LLMProvider: Send + Sync {
    fn name(&self) -> &str;
    fn provider_type(&self) -> &str; // e.g., "nvidia", "google", "openai"

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse>;
    async fn stream(&self, request: CompletionRequest) -> Result<mpsc::Receiver<StreamChunk>>;
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
}

/// Provider pool with user-selectable providers
pub struct ProviderPool {
    providers: RwLock<HashMap<String, Arc<dyn LLMProvider>>>,
    active_provider: RwLock<String>,
    client: Client,
}

impl ProviderPool {
    pub fn new(config: ProvidersConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.request_timeout_secs))
            .build()
            .unwrap_or_else(|_| Client::new());

        let mut providers = HashMap::new();
        let default = config.active.clone();

        // Register all providers from config
        for entry in &config.providers {
            let p = OpenAICompatibleProvider::new(
                entry.name.clone(),
                entry.api_key.clone(),
                entry.api_base.clone(),
                client.clone(),
                entry.extra_headers.clone(),
            );
            let p: Arc<dyn LLMProvider> = Arc::new(p);
            providers.insert(entry.name.clone(), p);
        }

        Self {
            providers: RwLock::new(providers),
            active_provider: RwLock::new(default),
            client,
        }
    }

    /// Create a new provider from config (for dynamic adding)
    pub async fn add_provider(&self, entry: ProviderEntry) -> Result<()> {
        let provider = Arc::new(OpenAICompatibleProvider::new(
            entry.name.clone(),
            entry.api_key.clone(),
            entry.api_base.clone(),
            self.client.clone(),
            entry.extra_headers.clone(),
        ));

        self.providers
            .write()
            .await
            .insert(entry.name.clone(), provider);
        tracing::info!("Added provider: {}", entry.name);
        Ok(())
    }

    /// Set active provider by name
    pub async fn set_active(&self, name: &str) -> Result<()> {
        if !self.providers.read().await.contains_key(name) {
            let available = self.list_providers().await;
            return Err(anyhow!(
                "Provider '{}' not found. Available: {:?}",
                name,
                available
            ));
        }
        *self.active_provider.write().await = name.to_string();
        tracing::info!("Switched to provider: {}", name);
        Ok(())
    }

    /// Get current active provider
    pub async fn get_active(&self) -> Option<Arc<dyn LLMProvider>> {
        let active = self.active_provider.read().await;
        self.providers.read().await.get(&*active).cloned()
    }

    /// List all available providers
    pub async fn list_providers(&self) -> Vec<ProviderInfo> {
        self.providers
            .read()
            .await
            .iter()
            .map(|(name, p)| ProviderInfo {
                name: name.clone(),
                provider_type: p.provider_type().to_string(),
            })
            .collect()
    }

    /// Get default provider name
    pub async fn default_name(&self) -> String {
        self.active_provider.read().await.clone()
    }

    /// Complete request using the active provider
    pub async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse> {
        const MAX_RETRIES: u32 = 3;
        const BASE_DELAY_MS: u64 = 1000;

        // If user has set a custom provider override, use it directly
        if let (Some(base_url), Some(api_key)) = (&request.base_url, &request.api_key) {
            tracing::info!("Using user-overridden provider: {}", base_url);
            let custom_provider = OpenAICompatibleProvider {
                name: "user-override".into(),
                provider_type: "openai".into(),
                api_base: base_url.clone(),
                api_key: api_key.clone(),
                client: reqwest::Client::new(),
                extra_headers: std::collections::HashMap::new(),
            };
            for attempt in 0..MAX_RETRIES {
                match custom_provider.complete(request.clone()).await {
                    Ok(response) => return Ok(response),
                    Err(e) => {
                        if attempt < MAX_RETRIES - 1 {
                            let delay_ms = BASE_DELAY_MS * 2u64.pow(attempt);
                            tracing::warn!(
                                "User provider failed (attempt {}), retrying in {}ms: {}",
                                attempt + 1, delay_ms, e
                            );
                            time::sleep(Duration::from_millis(delay_ms)).await;
                        } else {
                            tracing::error!(
                                "User provider failed after {} attempts: {}",
                                MAX_RETRIES, e
                            );
                        }
                    }
                }
            }
            return Err(anyhow!("User-overridden provider failed after {} attempts", MAX_RETRIES));
        }

        // Default provider pool path
        let provider = self
            .get_active()
            .await
            .ok_or_else(|| anyhow!("No active provider"))?;

        for attempt in 0..MAX_RETRIES {
            match provider.complete(request.clone()).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    if attempt < MAX_RETRIES - 1 {
                        let delay_ms = BASE_DELAY_MS * 2u64.pow(attempt);
                        tracing::warn!(
                            "Provider {} failed (attempt {}), retrying in {}ms: {}",
                            provider.name(),
                            attempt + 1,
                            delay_ms,
                            e
                        );
                        time::sleep(Duration::from_millis(delay_ms)).await;
                    } else {
                        tracing::error!(
                            "Provider {} failed after {} attempts: {}",
                            provider.name(),
                            MAX_RETRIES,
                            e
                        );
                    }
                }
            }
        }

        Err(anyhow!("Provider {} failed", provider.name()))
    }

    pub async fn stream(&self, request: CompletionRequest) -> Result<mpsc::Receiver<StreamChunk>> {
        let provider = self
            .get_active()
            .await
            .ok_or_else(|| anyhow!("No active provider"))?;
        provider.stream(request).await
    }
}

/// Info about a provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub name: String,
    pub provider_type: String,
}

/// OpenAI-compatible provider (NVIDIA, OpenAI, OpenRouter, Google, etc.)
pub struct OpenAICompatibleProvider {
    name: String,
    provider_type: String,
    api_key: String,
    api_base: String,
    client: Client,
    extra_headers: HashMap<String, String>,
}

impl OpenAICompatibleProvider {
    pub fn new(
        name: String,
        api_key: String,
        api_base: String,
        client: Client,
        extra_headers: Option<HashMap<String, String>>,
    ) -> Self {
        // Determine provider type from API base
        let provider_type = if api_base.contains("nvidia") {
            "nvidia"
        } else if api_base.contains("google") || api_base.contains("generativelanguage") {
            "google"
        } else if api_base.contains("openai") {
            "openai"
        } else if api_base.contains("openrouter") {
            "openrouter"
        } else if api_base.contains("groq") {
            "groq"
        } else if api_base.contains("deepseek") {
            "deepseek"
        } else {
            "openai-compatible"
        };

        Self {
            name,
            provider_type: provider_type.to_string(),
            api_key,
            api_base,
            client,
            extra_headers: extra_headers.unwrap_or_default(),
        }
    }

    fn build_headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", self.api_key).parse().unwrap(),
        );
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            "application/json".parse().unwrap(),
        );
        for (key, value) in &self.extra_headers {
            if let (Ok(name), Ok(val)) = (
                key.parse::<reqwest::header::HeaderName>(),
                value.parse::<reqwest::header::HeaderValue>(),
            ) {
                headers.insert(name, val);
            }
        }
        headers
    }
}

#[async_trait]
impl LLMProvider for OpenAICompatibleProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn provider_type(&self) -> &str {
        &self.provider_type
    }

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse> {
        // Strip provider prefix only for Google (they don't accept it)
        let model_name = if self.provider_type == "google" {
            request
                .model
                .split('/')
                .last()
                .unwrap_or(&request.model)
                .to_string()
        } else {
            request.model.clone()
        };

        let base = self.api_base.trim_end_matches('/');
        let mut url = format!("{}/chat/completions", base);

        // Google AI Studio requires key as query param
        if self.provider_type == "google" && !self.api_key.is_empty() {
            url = format!("{}?key={}", url, self.api_key);
        }

        let headers = if self.provider_type == "google" {
            let mut h = reqwest::header::HeaderMap::new();
            h.insert(
                reqwest::header::CONTENT_TYPE,
                "application/json".parse().unwrap(),
            );
            if !self.api_key.is_empty() {
                h.insert(
                    reqwest::header::AUTHORIZATION,
                    format!("Bearer {}", self.api_key).parse().unwrap(),
                );
            }
            h
        } else {
            self.build_headers()
        };

        tracing::debug!("Provider {} making request to: {}", self.name, url);

        let mut req = request.clone();
        req.model = model_name.to_string();

        // Ensure at least one system message exists at position 0
        // Some providers (Google, DeepSeek, certain OpenRouter models) return HTTP 400 without it
        if req.messages.first().map(|m| m.role.as_str()) != Some("system") {
            req.messages.insert(0, Message {
                role: "system".to_string(),
                content: Some("You are a helpful assistant.".to_string()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            });
            tracing::warn!(
                "Provider {} request missing system message -- injected default",
                self.name
            );
        }

        let body = serde_json::to_value(&req)?;

        let msg_count = req.messages.len();
        let tool_count = req.tools.as_ref().map(|t| t.len()).unwrap_or(0);
        let payload_bytes = body.to_string().len();
        tracing::info!(
            "Provider {} request: {} messages, {} tools, {} bytes payload -> {}",
            self.name, msg_count, tool_count, payload_bytes, url
        );

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                tracing::error!("Provider {} connection/send error: {:#}", self.name, e);
                e
            })?;

        let status = response.status();
        tracing::info!("Provider {} response status: {}", self.name, status);

        let response_body = response.text().await.map_err(|e| {
            tracing::error!("Provider {} failed to read response body: {:#}", self.name, e);
            e
        })?;

        if !status.is_success() {
            tracing::error!(
                "Provider {} HTTP {} error. Full response body:\n{}",
                self.name, status, response_body
            );
            if capture_provider_rejections_enabled() {
                match capture_rejected_request(&self.name, status.as_u16(), &response_body, &body) {
                    Ok(path) => tracing::error!(
                        "Provider {} rejected request captured at {}",
                        self.name,
                        path.display()
                    ),
                    Err(capture_error) => tracing::error!(
                        "Provider {} failed to capture rejected request: {}",
                        self.name,
                        capture_error
                    ),
                }
            }
            return Err(anyhow!("Provider {} HTTP {}: {}", self.name, status, response_body));
        }

        // Check for error responses that return HTTP 200 with error in body (some providers do this)
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&response_body) {
            if val.get("object").and_then(|o| o.as_str()) == Some("error")
                || val.get("error").is_some() && !val.get("choices").is_some()
            {
                let err_msg = val.get("message")
                    .or_else(|| val.get("error").and_then(|e| e.get("message")))
                    .and_then(|m| m.as_str())
                    .unwrap_or("Unknown provider error");
                let err_code = val.get("code")
                    .or_else(|| val.get("error").and_then(|e| e.get("code")))
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "unknown".into());
                tracing::error!(
                    "Provider {} returned error in body (HTTP {}): code={} message={}\nFull response: {}",
                    self.name, status, err_code, err_msg, response_body
                );
                return Err(anyhow!("Provider {} error (code={}): {}", self.name, err_code, err_msg));
            }
        }

        let completion: OpenAICompletion = match serde_json::from_str(&response_body) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(
                    "Provider {} JSON parse error: {:#}\nFull response body:\n{}",
                    self.name, e, response_body
                );
                return Err(anyhow!("Provider {} JSON parse error: {:#}", self.name, e));
            }
        };

        if let Some(err) = &completion.error {
            let err_msg = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            let err_code = err.get("code").and_then(|c| c.as_str()).unwrap_or("none");
            let err_status = err.get("status").and_then(|s| s.as_str()).unwrap_or("none");
            tracing::error!(
                "Provider {} returned error in response body: code={} status={} message={}\nFull error object: {}",
                self.name, err_code, err_status, err_msg,
                serde_json::to_string(err).unwrap_or_default()
            );
            return Err(anyhow!("Provider {} error (code={}, status={}): {}",
                self.name, err_code, err_status, err_msg));
        }

        let content = completion
            .choices
            .first()
            .map(|c| {
                c.message
                    .content
                    .clone()
                    .or(c.message.reasoning_content.clone())
                    .or(c.message.reasoning.clone())
                    .unwrap_or_default()
            })
            .unwrap_or_default();

        if completion.choices.is_empty() {
            tracing::error!(
                "Provider {} returned empty choices array. Usage: {:?}\nRaw response: {}",
                self.name, completion.usage, response_body
            );
        }

        if let Some(ref usage) = completion.usage {
            tracing::info!(
                "Provider {} completion: prompt_tokens={} completion_tokens={} total_tokens={}",
                self.name, usage.prompt_tokens, usage.completion_tokens, usage.total_tokens
            );
        }

        Ok(CompletionResponse {
            content,
            tool_calls: completion
                .choices
                .first()
                .and_then(|c| c.message.tool_calls.clone()),
            usage: completion.usage.map(|u| Usage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
            }),
        })
    }

    async fn stream(&self, request: CompletionRequest) -> Result<mpsc::Receiver<StreamChunk>> {
        let (tx, rx) = mpsc::channel(100);

        let base = self.api_base.trim_end_matches('/');
        let url = format!("{}/chat/completions", base);
        let mut stream_request = request;
        stream_request.stream = Some(true);

        let body = serde_json::to_value(&stream_request)?;
        let headers = self.build_headers();
        let client = self.client.clone();
        let provider_name = self.name.clone();

        tokio::spawn(async move {
            match client.post(&url).headers(headers).json(&body).send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        use futures::StreamExt;
                        let mut stream = response.bytes_stream();
                        while let Some(chunk) = stream.next().await {
                            if let Ok(bytes) = chunk {
                                let text = String::from_utf8_lossy(&bytes);
                                for line in text.lines() {
                                    if line.starts_with("data: ") {
                                        let data = &line[6..];
                                        if data == "[DONE]" {
                                            let _ = tx
                                                .send(StreamChunk {
                                                    id: None,
                                                    choices: vec![],
                                                    done: true,
                                                })
                                                .await;
                                            break;
                                        }
                                        if let Ok(parsed) =
                                            serde_json::from_str::<StreamChunk>(data)
                                        {
                                            let _ = tx.send(parsed).await;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Provider {} stream error: {}", provider_name, e);
                }
            }
        });

        Ok(rx)
    }

    async fn embed(&self, _text: &str) -> Result<Vec<f32>> {
        Err(anyhow!("Embedding not implemented"))
    }
}

fn capture_provider_rejections_enabled() -> bool {
    matches!(
        std::env::var("AUXLOCLAW_CAPTURE_REJECTED_REQUESTS")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn rejected_request_dir() -> PathBuf {
    std::env::var("AUXLOCLAW_REJECTED_REQUEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".auxloclaw/debug/rejected-requests")
        })
}

fn capture_rejected_request(
    provider: &str,
    status: u16,
    error: &str,
    body: &serde_json::Value,
) -> Result<PathBuf> {
    let dir = rejected_request_dir();
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;

    let body_bytes = serde_json::to_vec(body)?;
    let mut hasher = Sha256::new();
    hasher.update(&body_bytes);
    let hash = format!("{:x}", hasher.finalize());
    let hash_short = &hash[..16];
    let unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let safe_provider: String = provider
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let path = dir.join(format!("{}-{}-{}.json", unix_ms, safe_provider, hash_short));

    let capture = serde_json::json!({
        "captured_at_unix_ms": unix_ms,
        "provider": provider,
        "status": status,
        "error": error,
        "request_sha256": hash,
        "request_bytes": body_bytes.len(),
        "request": body,
    });

    let mut file =
        fs::File::create(&path).with_context(|| format!("creating {}", path.display()))?;
    file.write_all(serde_json::to_string_pretty(&capture)?.as_bytes())?;
    file.write_all(
        b"
",
    )?;
    Ok(path)
}

// ========== Request/Response Types ==========

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// User override: custom base URL for this request (bypasses provider pool)
    #[serde(skip)]
    pub base_url: Option<String>,
    /// User override: custom API key for this request (bypasses provider pool)
    #[serde(skip)]
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl Message {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn with_tool_calls(role: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: role.into(),
            content: None,
            tool_calls: Some(tool_calls),
            tool_call_id: None,
            name: None,
        }
    }

    pub fn tool_result(
        tool_call_id: impl Into<String>,
        name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            role: "tool".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
            name: Some(name.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone)]
pub struct CompletionResponse {
    pub content: String,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChunk {
    pub id: Option<String>,
    pub choices: Vec<StreamChoice>,
    #[serde(default)]
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChoice {
    pub index: u32,
    pub delta: StreamDelta,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamDelta {
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCall>>,
}

// ========== OpenAI Response Types ==========

#[derive(Debug, Deserialize)]
struct OpenAICompletion {
    choices: Vec<OpenAIChoice>,
    #[serde(default)]
    usage: Option<OpenAIUsage>,
    #[serde(default)]
    error: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAIMessage {
    content: Option<String>,
    tool_calls: Option<Vec<ToolCall>>,
    reasoning: Option<String>,
    reasoning_content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAIUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}