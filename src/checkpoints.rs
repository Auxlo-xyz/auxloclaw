//! Checkpoints and Session Rollback Module.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::memory::SessionHistory;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: String,
    pub timestamp: u64,
    pub session_snapshot: SessionHistory,
    pub metadata: HashMap<String, String>,
}

pub struct CheckpointManager {
    data_dir: PathBuf,
}

impl CheckpointManager {
    pub fn new(db_path: &str) -> Result<Self> {
        let data_dir = PathBuf::from(db_path)
            .parent()
            .map(|p| p.join("checkpoints"))
            .unwrap_or_else(|| PathBuf::from("checkpoints"));

        fs::create_dir_all(&data_dir)
            .with_context(|| format!("Failed to create checkpoints directory: {:?}", data_dir))?;

        Ok(Self { data_dir })
    }

    pub fn create_checkpoint(
        &self,
        session_id: &str,
        history: &SessionHistory,
        label: Option<&str>,
    ) -> Result<String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        let id = format!("{}_{}", session_id, now);
        let checkpoint = Checkpoint {
            id: id.clone(),
            timestamp: now,
            session_snapshot: history.clone(),
            metadata: {
                let mut m = HashMap::new();
                if let Some(l) = label {
                    m.insert("label".into(), l.into());
                }
                m
            },
        };

        let path = self.data_dir.join(format!("{}.json", id));
        let json = serde_json::to_string_pretty(&checkpoint)?;
        fs::write(&path, json)?;

        Ok(id)
    }

    pub fn rollback(&self, session_id: &str, checkpoint_id: &str) -> Result<SessionHistory> {
        let path = self.data_dir.join(format!("{}.json", checkpoint_id));
        if !path.exists() {
            return Err(anyhow::anyhow!("Checkpoint not found: {}", checkpoint_id));
        }

        let json = fs::read_to_string(&path)?;
        let checkpoint: Checkpoint = serde_json::from_str(&json)?;

        // Verify it belongs to this session
        if !checkpoint.id.starts_with(session_id) {
            return Err(anyhow::anyhow!(
                "Checkpoint does not belong to session {}",
                session_id
            ));
        }

        Ok(checkpoint.session_snapshot)
    }

    pub fn list_checkpoints(&self, session_id: &str) -> Result<Vec<(String, u64, Option<String>)>> {
        let mut list = Vec::new();
        if !self.data_dir.exists() {
            return Ok(list);
        }

        for entry in fs::read_dir(&self.data_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Ok(json) = fs::read_to_string(&path) {
                    if let Ok(cp) = serde_json::from_str::<Checkpoint>(&json) {
                        if cp.id.starts_with(session_id) {
                            list.push((cp.id, cp.timestamp, cp.metadata.get("label").cloned()));
                        }
                    }
                }
            }
        }
        list.sort_by(|a, b| b.1.cmp(&a.1));
        Ok(list)
    }
}
