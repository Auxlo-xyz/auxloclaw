//! Provider pool with connection multiplexing and fallback support

use anyhow::{anyhow, Result};
use async_trait::async_trait;

use dashmap::DashMap;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time;

use crate::config::{ProviderEntry, ProvidersConfig};

/// LLM Provider trait - unified interface for all providers
#[async_trait]
pub trait LLMProvider: Send + Sync {
    fn name(&self) -> &str;
    
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse>;
    
    async fn stream(&self, request: CompletionRequest) -> Result<mpsc::Receiver<StreamChunk>>;
    
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
}

/// Provider pool with load balancing and fallback
pub struct ProviderPool {
    primary: Arc<dyn LLMProvider>,
    fallbacks: Vec<Arc<dyn LLMProvider>>,
    #[allow(dead_code)]
    client: Client,
}

impl ProviderPool {
    pub fn new(config: ProvidersConfig) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(config.request_timeout_secs))
            .build()
            .unwrap_or_else(|_| Client::new());
        
        // Create primary provider
        let primary: Arc<dyn LLMProvider> = Arc::new(OpenAICompatibleProvider::new(
            config.primary.name.clone(),
            config.primary.api_key.clone(),
            config.primary.api_base.clone(),
            client.clone(),
            config.primary.extra_headers.clone(),
        ));
        
        // Create fallback providers
        let fallbacks: Vec<Arc<dyn LLMProvider>> = config.fallbacks
            .iter()
            .filter_map(|f| {
                Some(Arc::new(OpenAICompatibleProvider::new(
                    f.name.clone(),
                    f.api_key.clone(),
                    f.api_base.clone(),
                    client.clone(),
                    f.extra_headers.clone(),
                )) as Arc<dyn LLMProvider>)
            })
            .collect();
        
        Self {
            primary,
            fallbacks,
            client,
        }
    }

    fn create_provider(config: &ProviderEntry, client: Client) -> Result<Box<dyn LLMProvider>> {
        match config.name.to_lowercase().as_str() {
            "nvidia" | "openai" | "openrouter" | "deepseek" | "groq" | "anthropic" => {
                Ok(Box::new(OpenAICompatibleProvider::new(
                    config.name.clone(),
                    config.api_key.clone(),
                    config.api_base.clone(),
                    client,
                    config.extra_headers.clone(),
                )))
            }
            _ => Err(anyhow!("Unknown provider: {}", config.name)),
        }
    }

    pub async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse> {
        const MAX_RETRIES: u32 = 3;
        const BASE_DELAY_MS: u64 = 1000;

        // Try primary with retries
        for attempt in 0..MAX_RETRIES {
            match self.primary.complete(request.clone()).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    if attempt < MAX_RETRIES - 1 {
                        let delay_ms = BASE_DELAY_MS * 2u64.pow(attempt);
                        tracing::warn!("Provider failed (attempt {}), retrying in {}ms: {}", attempt + 1, delay_ms, e);
                        time::sleep(Duration::from_millis(delay_ms)).await;
                    } else {
                        tracing::warn!("Primary provider failed after {} attempts: {}", MAX_RETRIES, e);
                    }
                }
            }
        }

        // Try fallbacks
        for fallback in &self.fallbacks {
            match fallback.complete(request.clone()).await {
                Ok(response) => {
                    tracing::info!("Fallback provider {} succeeded", fallback.name());
                    return Ok(response);
                }
                Err(e) => {
                    tracing::warn!("Fallback {} failed: {}", fallback.name(), e);
                }
            }
        }

        Err(anyhow!("All providers failed"))
    }

    pub async fn stream(&self, request: CompletionRequest) -> Result<mpsc::Receiver<StreamChunk>> {
        self.primary.stream(request).await
    }

    pub fn primary(&self) -> &Arc<dyn LLMProvider> {
        &self.primary
    }
}

/// OpenAI-compatible provider (NVIDIA, OpenAI, OpenRouter, etc.)
pub struct OpenAICompatibleProvider {
    name: String,
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
        Self {
            name,
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
            if let Ok(name) = key.parse::<reqwest::header::HeaderName>() {
                if let Ok(val) = value.parse::<reqwest::header::HeaderValue>() {
                    headers.insert(name, val);
                }
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

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse> {
        // Strip provider prefix from model name (e.g., "google/gemma-4-26b-a4b-it" -> "gemma-4-26b-a4b-it")
        let model_name = request.model.split('/').last().unwrap_or(&request.model);
        
        let mut url = format!("{}/chat/completions", self.api_base);
        
        // Google AI Studio requires key as query param
        if self.api_base.contains("generativelanguage.googleapis.com") && !self.api_key.is_empty() {
            url = format!("{}?key={}", url, self.api_key);
        }
        
        // Build headers - Google needs Authorization header too
        let headers = if self.api_base.contains("generativelanguage.googleapis.com") {
            let mut h = reqwest::header::HeaderMap::new();
            h.insert(reqwest::header::CONTENT_TYPE, "application/json".parse().unwrap());
            // Google also needs Bearer auth even with key param
            if !self.api_key.is_empty() {
                h.insert(reqwest::header::AUTHORIZATION, format!("Bearer {}", self.api_key).parse().unwrap());
            }
            h
        } else {
            self.build_headers()
        };
        
        tracing::info!("Making request to URL: {} with key present: {}", url, !self.api_key.is_empty());
        
        let mut req = request.clone();
        req.model = model_name.to_string();
        let body = serde_json::to_value(&req)?;
        tracing::debug!("Request body: {}", serde_json::to_string_pretty(&body)?);
        
        let response = self.client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        tracing::debug!("Response status: {}", status);
        
        if !status.is_success() {
            let error = response.text().await?;
            tracing::error!("API error: {}", error);
            return Err(anyhow!("API error: {}", error));
        }

        let completion: OpenAICompletion = response.json().await?;
        
        // Check if Google returned an error in the response body
        if let Some(err) = completion.error {
            let err_msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("Unknown error");
            tracing::error!("API returned error: {}", err_msg);
            return Err(anyhow!("API error: {}", err_msg));
        }
        
        Ok(CompletionResponse {
            content: completion.choices
                .first()
                .map(|c| c.message.content.clone().or(c.message.reasoning.clone()).unwrap_or_default())
                .unwrap_or_default(),
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
        let mut stream_request = request.clone();
        stream_request.stream = Some(true);
        
        let body = serde_json::to_value(&stream_request)?;
        let headers = self.build_headers();
        let client = self.client.clone();
        
        tokio::spawn(async move {
            match client
                .post(&url)
                .headers(headers)
                .json(&body)
                .send()
                .await
            {
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
                                            let _ = tx.send(StreamChunk { 
                                                id: None, 
                                                choices: vec![], 
                                                done: true 
                                            }).await;
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
                    tracing::error!("Stream error: {}", e);
                }
            }
        });

        Ok(rx)
    }

    async fn embed(&self, _text: &str) -> Result<Vec<f32>> {
        Err(anyhow!("Embedding not implemented for this provider"))
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