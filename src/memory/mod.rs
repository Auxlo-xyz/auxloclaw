//! Memory Engine - with JSON file persistence for sessions

use anyhow::{Context, Result};
use lru::LruCache;
use std::sync::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;
use std::fs;

use crate::config::MemoryConfig;

pub mod compactor;
pub mod reflector;

pub use compactor::{Compactor, CompactionResult, CompactionSummary};
pub use reflector::{Reflector, Reflection, ReflectionType, ReflectorConfig};

/// Memory entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub key: String,
    pub content: String,
    pub metadata: HashMap<String, String>,
    pub created_at: u64,
    pub accessed_at: u64,
    pub access_count: u32,
}

/// Simplified memory engine (hot cache only for now)
pub struct MemoryEngine {
    hot: RwLock<LruCache<String, MemoryEntry>>,
    #[allow(dead_code)]
    db_path: PathBuf,
}

impl MemoryEngine {
    pub fn new(config: &MemoryConfig) -> Result<Self> {
        let hot = LruCache::new(NonZeroUsize::new(config.hot_cache_size).unwrap());
        let db_path = PathBuf::from(&config.database_path);
        
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create memory directory: {:?}", parent))?;
        }
        
        Ok(Self {
            hot: RwLock::new(hot),
            db_path,
        })
    }

    pub fn store(&self, key: &str, content: &str, metadata: Option<HashMap<String, String>>) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        let entry = MemoryEntry {
            key: key.to_string(),
            content: content.to_string(),
            metadata: metadata.unwrap_or_default(),
            created_at: now,
            accessed_at: now,
            access_count: 1,
        };

        let mut hot = self.hot.write().unwrap();
        hot.put(key.to_string(), entry);
        
        Ok(())
    }

    pub fn retrieve(&self, key: &str) -> Option<MemoryEntry> {
        let mut hot = self.hot.write().unwrap();
        if let Some(entry) = hot.get_mut(key) {
            entry.accessed_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_secs();
            entry.access_count += 1;
            return Some(entry.clone());
        }
        None
    }

    pub fn hot_keys(&self) -> Vec<String> {
        let hot = self.hot.read().unwrap();
        hot.iter().map(|(k, _)| k.clone()).collect()
    }

    pub fn clear_hot(&self) {
        let mut hot = self.hot.write().unwrap();
        hot.clear();
    }
}

/// Session history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHistory {
    pub session_id: String,
    pub messages: Vec<HistoryMessage>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryMessage {
    pub role: String,
    pub content: String,
    pub timestamp: u64,
    #[serde(default)]
    pub tool_calls: Option<Vec<serde_json::Value>>,
}

impl SessionHistory {
    pub fn new(session_id: &str) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        Self {
            session_id: session_id.to_string(),
            messages: vec![],
            created_at: now,
            updated_at: now,
        }
    }

    pub fn add_message(&mut self, role: &str, content: &str, tool_calls: Option<Vec<serde_json::Value>>) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        self.messages.push(HistoryMessage {
            role: role.to_string(),
            content: content.to_string(),
            timestamp: now,
            tool_calls,
        });
        self.updated_at = now;
    }
}

/// Persistent session store using JSON files
pub struct SessionStore {
    data_dir: PathBuf,
}

impl SessionStore {
    pub fn new(db_path: &str) -> Result<Self> {
        let data_dir = PathBuf::from(db_path)
            .parent()
            .map(|p| p.join("sessions"))
            .unwrap_or_else(|| PathBuf::from("sessions"));
        
        fs::create_dir_all(&data_dir)
            .with_context(|| format!("Failed to create session directory: {:?}", data_dir))?;
        
        Ok(Self { data_dir })
    }
    
    fn session_path(&self, session_id: &str) -> PathBuf {
        // Sanitize session_id for filesystem
        let safe_id = session_id.replace(['/', '\\', ':'], "_");
        self.data_dir.join(format!("{}.json", safe_id))
    }
    
    /// Save a session to disk
    pub fn save(&self, session_id: &str, history: &SessionHistory) -> Result<()> {
        let path = self.session_path(session_id);
        let json = serde_json::to_string_pretty(history)?;
        fs::write(&path, json)
            .with_context(|| format!("Failed to write session file: {:?}", path))?;
        Ok(())
    }
    
    /// Load a session from disk
    pub fn load(&self, session_id: &str) -> Result<Option<SessionHistory>> {
        let path = self.session_path(session_id);
        if !path.exists() {
            return Ok(None);
        }
        
        let json = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read session file: {:?}", path))?;
        let history: SessionHistory = serde_json::from_str(&json)?;
        Ok(Some(history))
    }
    
    /// Load all sessions from disk
    pub fn load_all(&self) -> Result<Vec<(String, SessionHistory)>> {
        let mut result = Vec::new();
        
        if !self.data_dir.exists() {
            return Ok(result);
        }
        
        for entry in fs::read_dir(&self.data_dir)? {
            let entry = entry?;
            let path = entry.path();
            
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Ok(json) = fs::read_to_string(&path) {
                    if let Ok(history) = serde_json::from_str::<SessionHistory>(&json) {
                        result.push((history.session_id.clone(), history));
                    }
                }
            }
        }
        
        // Sort by updated_at descending
        result.sort_by(|a, b| b.1.updated_at.cmp(&a.1.updated_at));
        
        Ok(result)
    }
    
    /// Delete a session from disk
    pub fn delete(&self, session_id: &str) -> Result<()> {
        let path = self.session_path(session_id);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("Failed to delete session file: {:?}", path))?;
        }
        Ok(())
    }
    
    /// Get session count
    pub fn count(&self) -> Result<usize> {
        if !self.data_dir.exists() {
            return Ok(0);
        }
        
        let count = fs::read_dir(&self.data_dir)?
            .filter(|e| {
                e.as_ref().ok()
                    .and_then(|e| e.path().extension().map(|e| e == "json"))
                    .unwrap_or(false)
            })
            .count();
        
        Ok(count)
    }
}
