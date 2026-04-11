//! Provider pool with multi-provider support
//! Users can choose which provider/model to use at any time

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use dashmap::DashMap;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};
use tokio::time;

use crate::config::{ProviderEntry, ProvidersConfig};

/// LLM Provider trait - unified interface for all providers
#[async_trait]
pub trait LLMProvider: Send + Sync {
    fn name(&self) -> &str;
    fn provider_type(&self) -> &str;  // e.g., "nvidia", "google", "openai"
    
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
        
        self.providers.write().await.insert(entry.name.clone(), provider);
        tracing::info!("Added provider: {}", entry.name);
        Ok(())
    }
    
    /// Set active provider by name
    pub async fn set_active(&self, name: &str) -> Result<()> {
        if !self.providers.read().await.contains_key(name) {
            let available = self.list_providers().await;
            return Err(anyhow!("Provider '{}' not found. Available: {:?}", name, available));
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
        self.providers.read().await.iter().map(|(name, p)| {
            ProviderInfo {
                name: name.clone(),
                provider_type: p.provider_type().to_string(),
            }
        }).collect()
    }
    
    /// Get default provider name
    pub async fn default_name(&self) -> String {
        self.active_provider.read().await.clone()
    }

    /// Complete request using the active provider
    pub async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse> {
        const MAX_RETRIES: u32 = 3;
        const BASE_DELAY_MS: u64 = 1000;
        
        let provider = self.get_active().await
            .ok_or_else(|| anyhow!("No active provider"))?;
        
        for attempt in 0..MAX_RETRIES {
            match provider.complete(request.clone()).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    if attempt < MAX_RETRIES - 1 {
                        let delay_ms = BASE_DELAY_MS * 2u64.pow(attempt);
                        tracing::warn!("Provider {} failed (attempt {}), retrying in {}ms: {}", 
                            provider.name(), attempt + 1, delay_ms, e);
                        time::sleep(Duration::from_millis(delay_ms)).await;
                    } else {
                        tracing::error!("Provider {} failed after {} attempts: {}", 
                            provider.name(), MAX_RETRIES, e);
                    }
                }
            }
        }
        
        Err(anyhow!("Provider {} failed", provider.name()))
    }

    pub async fn stream(&self, request: CompletionRequest) -> Result<mpsc::Receiver<StreamChunk>> {
        let provider = self.get_active().await
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
                value.parse::<reqwest::header::HeaderValue>()
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
            request.model.split('/').last().unwrap_or(&request.model).to_string()
        } else {
            request.model.clone()
        };
        
        let mut url = format!("{}/chat/completions", self.api_base);
        
        // Google AI Studio requires key as query param
        if self.provider_type == "google" && !self.api_key.is_empty() {
            url = format!("{}?key={}", url, self.api_key);
        }
        
        let headers = if self.provider_type == "google" {
            let mut h = reqwest::header::HeaderMap::new();
            h.insert(reqwest::header::CONTENT_TYPE, "application/json".parse().unwrap());
            if !self.api_key.is_empty() {
                h.insert(reqwest::header::AUTHORIZATION, format!("Bearer {}", self.api_key).parse().unwrap());
            }
            h
        } else {
            self.build_headers()
        };
        
        tracing::debug!("Provider {} making request to: {}", self.name, url);
        
        let mut req = request.clone();
        req.model = model_name.to_string();
        let body = serde_json::to_value(&req)?;
        
        let response = self.client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        tracing::debug!("Provider {} response status: {}", self.name, status);
        
        if !status.is_success() {
            let error = response.text().await?;
            tracing::error!("Provider {} API error: {}", self.name, error);
            return Err(anyhow!("API error: {}", error));
        }

        let completion: OpenAICompletion = response.json().await?;
        
        // Check for Google error in response
        if let Some(err) = completion.error {
            let err_msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("Unknown error");
            return Err(anyhow!("API error: {}", err_msg));
        }
        
        let content = completion.choices
            .first()
            .map(|c| {
                c.message.content.clone()
                    .or(c.message.reasoning.clone())
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        
        Ok(CompletionResponse {
            content,
            tool_calls: completion.choices
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
        
        let url = format!("{}/chat/completions", self.api_base);
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
                                            let _ = tx.send(StreamChunk { id: None, choices: vec![], done: true }).await;
                                            break;
                                        }
                                        if let Ok(parsed) = serde_json::from_str::<StreamChunk>(data) {
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
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
}

#[derive(Debug, Deserialize)]
struct OpenAIUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}