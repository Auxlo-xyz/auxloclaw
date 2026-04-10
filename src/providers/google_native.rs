//! Google Native API Provider for Gemma models

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;

use super::{CompletionRequest, CompletionResponse, LLMProvider, StreamChunk};

#[derive(Debug, Deserialize)]
struct GoogleGenerateContentResponse {
    candidates: Option<Vec<GoogleCandidate>>,
    #[serde(default)]
    error: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct GoogleCandidate {
    content: Option<GoogleContent>,
}

#[derive(Debug, Deserialize)]
struct GoogleContent {
    parts: Vec<GooglePart>,
}

#[derive(Debug, Deserialize, Serialize)]
struct GooglePart {
    text: String,
    #[serde(default)]
    thought: bool,
}

pub struct GoogleNativeProvider {
    api_key: String,
    client: Client,
}

impl GoogleNativeProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: Client::new(),
        }
    }
}

#[async_trait]
impl LLMProvider for GoogleNativeProvider {
    fn name(&self) -> &str {
        "google-native"
    }

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse> {
        let model_name = request.model.split('/').last().unwrap_or(&request.model);
        
        // Convert messages to Google format
        let text = request.messages.iter()
            .filter(|m| m.role != "system")
            .filter_map(|m| {
                let content = m.content.trim();
                if content.is_empty() { None } else { Some(content.to_string()) }
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            model_name, self.api_key
        );
        
        #[derive(Serialize)]
        struct GenerateContentRequest {
            contents: Vec<GoogleContentRequest>,
        }
        
        #[derive(Serialize)]
        struct GoogleContentRequest {
            parts: Vec<GooglePartRequest>,
        }
        
        #[derive(Serialize)]
        struct GooglePartRequest {
            text: String,
        }
        
        let google_req = GenerateContentRequest {
            contents: vec![GoogleContentRequest {
                parts: vec![GooglePartRequest { text }],
            }],
        };
        
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::CONTENT_TYPE, "application/json".parse().unwrap());
        
        let response = self.client
            .post(&url)
            .headers(headers)
            .json(&google_req)
            .send()
            .await?;

        if !response.status().is_success() {
            let error = response.text().await?;
            tracing::error!("Google API error: {}", error);
            return Err(anyhow!("API error: {}", error));
        }

        let google_resp: GoogleGenerateContentResponse = response.json().await?;
        
        if let Some(err) = google_resp.error {
            let err_msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("Unknown error");
            return Err(anyhow!("API error: {}", err_msg));
        }
        
        // Filter out thought parts and extract only the actual response text
        let content = google_resp.candidates
            .and_then(|c| c.into_iter().next())
            .and_then(|c| c.content)
            .map(|c| {
                c.parts.into_iter()
                    .filter(|p| !p.thought)
                    .filter_map(|p| if p.text.is_empty() { None } else { Some(p.text) })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        
        Ok(CompletionResponse {
            content,
            tool_calls: None,
            usage: None,
        })
    }

    async fn stream(&self, _request: CompletionRequest) -> Result<mpsc::Receiver<StreamChunk>> {
        Err(anyhow!("Streaming not implemented for Google Native provider"))
    }

    async fn embed(&self, _text: &str) -> Result<Vec<f32>> {
        Err(anyhow!("Embedding not implemented for this provider"))
    }
}
