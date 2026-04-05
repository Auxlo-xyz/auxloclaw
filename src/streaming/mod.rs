//! Streaming module
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// Stream event
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StreamEvent {
    #[serde(rename = "token")]
    Token { content: String },
    #[serde(rename = "done")]
    Done { message: String },
    #[serde(rename = "error")]
    Error { message: String },
}

/// Stream request
#[derive(Debug, Deserialize)]
pub struct StreamRequest {
    pub message: String,
    pub session_id: Option<String>,
}

/// Stream session
pub struct StreamSession {
    #[allow(dead_code)]
    pub session_id: String,
    pub tx: mpsc::Sender<StreamEvent>,
    #[allow(dead_code)]
    pub started_at: std::time::Instant,
}

impl StreamSession {
    pub fn new(session_id: String, buffer_size: usize) -> (Self, mpsc::Receiver<StreamEvent>) {
        let (tx, rx) = mpsc::channel(buffer_size);
        (
            Self {
                session_id,
                tx,
                started_at: std::time::Instant::now(),
            },
            rx,
        )
    }

    pub async fn send_token(&self, content: &str) -> Result<(), mpsc::error::SendError<StreamEvent>> {
        self.tx.send(StreamEvent::Token { content: content.into() }).await
    }

    pub async fn send_done(&self, message: &str) -> Result<(), mpsc::error::SendError<StreamEvent>> {
        self.tx.send(StreamEvent::Done { message: message.into() }).await
    }
}