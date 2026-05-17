//! Reflection System - Produces structured session analysis
//! Similar to Claude Code's Auto Dream feature

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{HistoryMessage, SessionHistory};
use crate::config::MemoryConfig;

/// Reflection type classification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ReflectionType {
    Bugfix,
    Feature,
    Research,
    Question,
    Other,
}

impl std::fmt::Display for ReflectionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReflectionType::Bugfix => write!(f, "bugfix"),
            ReflectionType::Feature => write!(f, "feature"),
            ReflectionType::Research => write!(f, "research"),
            ReflectionType::Question => write!(f, "question"),
            ReflectionType::Other => write!(f, "other"),
        }
    }
}

/// Structured reflection output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reflection {
    #[serde(rename = "type")]
    pub reflection_type: ReflectionType,
    pub title: String,
    pub narrative: String,
    #[serde(rename = "userGoal")]
    pub user_goal: String,
    pub completed: String,
    #[serde(rename = "nextSteps")]
    pub next_steps: Vec<String>,
    pub session_id: String,
    pub message_count: usize,
    pub created_at: u64,
}

/// Reflector configuration
#[derive(Debug, Clone)]
pub struct ReflectorConfig {
    pub enabled: bool,
    pub min_messages: usize,
    pub cooldown_secs: u64,
    pub max_messages: usize,
    pub max_prompt_chars: usize,
}

impl Default for ReflectorConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_messages: 5,
            cooldown_secs: 300, // 5 minutes
            max_messages: 10,
            max_prompt_chars: 20_000,
        }
    }
}

/// Reflector - produces structured session analysis
pub struct Reflector {
    config: ReflectorConfig,
    reflections_dir: PathBuf,
    last_reflection: std::sync::RwLock<HashMap<String, u64>>,
}

use std::collections::HashMap;

impl Reflector {
    pub fn new(config: ReflectorConfig, data_dir: PathBuf) -> Self {
        let reflections_dir = data_dir.join("reflections");
        let _ = fs::create_dir_all(&reflections_dir);

        Self {
            config,
            reflections_dir,
            last_reflection: std::sync::RwLock::new(HashMap::new()),
        }
    }

    /// Check if reflection should run for a session
    pub fn should_reflect(&self, session_id: &str, message_count: usize) -> bool {
        if !self.config.enabled {
            return false;
        }

        if message_count < self.config.min_messages {
            return false;
        }

        // Check cooldown
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let last = self
            .last_reflection
            .read()
            .unwrap()
            .get(session_id)
            .copied()
            .unwrap_or(0);

        now.saturating_sub(last) >= self.config.cooldown_secs
    }

    /// Reflect on a session and produce structured output
    pub async fn reflect(&self, session: &SessionHistory) -> Result<Option<Reflection>> {
        if !self.should_reflect(&session.session_id, session.messages.len()) {
            return Ok(None);
        }

        let latest_reflection = self
            .load_reflections(&session.session_id)?
            .into_iter()
            .next();
        if let Some(existing) = latest_reflection.as_ref() {
            if existing.message_count >= session.messages.len() {
                self.mark_reflected(&session.session_id);
                return Ok(None);
            }
        }

        // Build prompt for reflection with bounded context.
        let prompt = self.build_reflection_prompt(&session.messages);
        if prompt.trim().is_empty() {
            self.mark_reflected(&session.session_id);
            return Ok(None);
        }

        // Call pollinations.ai for reflection
        let response = self.call_pollinations(&prompt).await?;

        // Parse the JSON response
        let reflection =
            self.parse_reflection(&response, &session.session_id, session.messages.len())?;

        if let Some(existing) = latest_reflection {
            if self.is_duplicate_reflection(&existing, &reflection) {
                self.mark_reflected(&session.session_id);
                return Ok(None);
            }
        }

        // Save reflection
        self.save_reflection(&reflection)?;
        self.mark_reflected(&session.session_id);

        Ok(Some(reflection))
    }

    fn mark_reflected(&self, session_id: &str) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        self.last_reflection
            .write()
            .unwrap()
            .insert(session_id.to_string(), now);
    }

    fn is_duplicate_reflection(&self, existing: &Reflection, next: &Reflection) -> bool {
        existing
            .title
            .trim()
            .eq_ignore_ascii_case(next.title.trim())
            && existing
                .user_goal
                .trim()
                .eq_ignore_ascii_case(next.user_goal.trim())
            && existing
                .completed
                .trim()
                .eq_ignore_ascii_case(next.completed.trim())
    }

    /// Build the reflection prompt
    fn build_reflection_prompt(&self, messages: &[HistoryMessage]) -> String {
        let recent_messages = self.recent_messages(messages);
        if recent_messages.is_empty() {
            return String::new();
        }

        let mut prompt = String::from(
            r#"Analyze this recent bounded conversation window and produce a structured reflection.

You MUST respond with ONLY valid JSON in this exact format:
{
  "type": "bugfix|feature|research|question|other",
  "title": "Short descriptive title",
  "narrative": "Brief narrative of what happened",
  "userGoal": "What the user was trying to accomplish",
  "completed": "What was successfully completed",
  "nextSteps": ["Step 1", "Step 2"]
}

Conversation:
"#,
        );

        for msg in recent_messages {
            let role = match msg.role.as_str() {
                "user" => "User",
                "assistant" => "Assistant",
                "system" => "System",
                "tool" => "Tool",
                _ => &msg.role,
            };

            // Truncate very long messages
            let content = if msg.content.len() > 500 {
                format!("{}... [truncated]", &msg.content[..500])
            } else {
                msg.content.clone()
            };

            let line = format!("{}: {}\n", role, content);
            if prompt.len() + line.len() > self.config.max_prompt_chars {
                prompt.push_str("[older content omitted to stay within reflection budget]\n");
                break;
            }
            prompt.push_str(&line);
        }

        prompt.push_str("\nRespond with ONLY the JSON object, no other text.");
        prompt
    }

    fn recent_messages<'a>(&self, messages: &'a [HistoryMessage]) -> Vec<&'a HistoryMessage> {
        let max = self.config.max_messages.max(1);
        let start = messages.len().saturating_sub(max);
        messages[start..].iter().collect()
    }

    /// Call pollinations.ai for reflection
    async fn call_pollinations(&self, prompt: &str) -> Result<String> {
        let body = serde_json::json!({
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ]
        });

        let client = reqwest::Client::new();
        let response = client
            .post("https://text.pollinations.ai/")
            .header("Content-Type", "application/json")
            .json(&body)
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await
            .context("Failed to call pollinations.ai")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Pollinations.ai error: {} - {}", status, body);
        }

        let text = response
            .text()
            .await
            .context("Failed to read pollinations.ai response")?;

        Ok(text)
    }

    /// Parse the reflection response
    fn parse_reflection(
        &self,
        response: &str,
        session_id: &str,
        message_count: usize,
    ) -> Result<Reflection> {
        // Try to extract JSON from the response
        let json_str = self.extract_json(response)?;

        // Parse the JSON
        let mut parsed: serde_json::Value = serde_json::from_str(&json_str)
            .with_context(|| format!("Failed to parse reflection JSON: {}", json_str))?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Build reflection with defaults for missing fields
        let reflection = Reflection {
            reflection_type: self.parse_type(&parsed["type"]),
            title: parsed["title"]
                .as_str()
                .unwrap_or("Untitled session")
                .to_string(),
            narrative: parsed["narrative"].as_str().unwrap_or("").to_string(),
            user_goal: parsed["userGoal"]
                .as_str()
                .unwrap_or("Unknown goal")
                .to_string(),
            completed: parsed["completed"].as_str().unwrap_or("").to_string(),
            next_steps: self.parse_next_steps(&parsed["nextSteps"]),
            session_id: session_id.to_string(),
            message_count,
            created_at: now,
        };

        Ok(reflection)
    }

    /// Extract JSON from response (handles markdown code blocks, etc.)
    fn extract_json(&self, response: &str) -> Result<String> {
        let trimmed = response.trim();

        // Try direct JSON parse first
        if trimmed.starts_with('{') {
            // Find the end of the JSON object
            if let Some(end) = trimmed.rfind('}') {
                return Ok(trimmed[..=end].to_string());
            }
        }

        // Try to extract from markdown code block
        if let Some(start) = trimmed.find("```json") {
            let rest = &trimmed[start + 7..];
            if let Some(end) = rest.find("```") {
                return Ok(rest[..end].trim().to_string());
            }
        }

        // Try to find JSON object anywhere
        if let Some(start) = trimmed.find('{') {
            let rest = &trimmed[start..];
            if let Some(end) = rest.rfind('}') {
                return Ok(rest[..=end].to_string());
            }
        }

        anyhow::bail!("Could not extract JSON from response: {}", trimmed)
    }

    /// Parse reflection type
    fn parse_type(&self, value: &serde_json::Value) -> ReflectionType {
        match value.as_str().map(|s| s.to_lowercase()).as_deref() {
            Some("bugfix") => ReflectionType::Bugfix,
            Some("feature") => ReflectionType::Feature,
            Some("research") => ReflectionType::Research,
            Some("question") => ReflectionType::Question,
            Some("other") | Some(_) | None => ReflectionType::Other,
        }
    }

    /// Parse next steps array
    fn parse_next_steps(&self, value: &serde_json::Value) -> Vec<String> {
        value
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Save reflection to disk
    fn save_reflection(&self, reflection: &Reflection) -> Result<()> {
        let filename = format!(
            "{}_{}.json",
            reflection.session_id.replace(['/', '\\', ':'], "_"),
            reflection.created_at
        );
        let path = self.reflections_dir.join(filename);
        let json = serde_json::to_string_pretty(reflection)?;
        fs::write(&path, json)
            .with_context(|| format!("Failed to write reflection file: {:?}", path))?;
        Ok(())
    }

    /// Load all reflections for a session
    pub fn load_reflections(&self, session_id: &str) -> Result<Vec<Reflection>> {
        let mut reflections = Vec::new();

        if !self.reflections_dir.exists() {
            return Ok(reflections);
        }

        let safe_id = session_id.replace(['/', '\\', ':'], "_");

        for entry in fs::read_dir(&self.reflections_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path
                .file_name()
                .map(|n| n.to_string_lossy().starts_with(&safe_id))
                .unwrap_or(false)
            {
                if let Ok(json) = fs::read_to_string(&path) {
                    if let Ok(reflection) = serde_json::from_str::<Reflection>(&json) {
                        reflections.push(reflection);
                    }
                }
            }
        }

        // Sort by created_at descending
        reflections.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        Ok(reflections)
    }

    /// Load all reflections across all sessions
    pub fn load_all_reflections(&self) -> Result<Vec<Reflection>> {
        let mut reflections = Vec::new();

        if !self.reflections_dir.exists() {
            return Ok(reflections);
        }

        for entry in fs::read_dir(&self.reflections_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Ok(json) = fs::read_to_string(&path) {
                    if let Ok(reflection) = serde_json::from_str::<Reflection>(&json) {
                        reflections.push(reflection);
                    }
                }
            }
        }

        // Sort by created_at descending
        reflections.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        Ok(reflections)
    }
}
