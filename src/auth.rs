//! Authentication and Authorization Module

use anyhow::{anyhow, Result};

/// Authentication configuration
#[derive(Clone)]
pub struct AuthConfig {
    pub api_key: Option<String>,
    pub require_auth: bool,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            require_auth: false,
        }
    }
}

/// Authentication state
pub struct AuthState {
    config: AuthConfig,
}

impl AuthState {
    pub fn new(config: AuthConfig) -> Self {
        Self { config }
    }

    /// Verify bearer token authentication
    pub fn verify_bearer_token(&self, auth_header: Option<&str>) -> Result<()> {
        if !self.config.require_auth {
            return Ok(());
        }

        let auth = auth_header.ok_or_else(|| anyhow!("Missing Authorization header"))?;

        if !auth.starts_with("Bearer ") {
            return Err(anyhow!("Invalid Authorization header format"));
        }

        let token = &auth[7..];

        match &self.config.api_key {
            Some(expected_key) if token == expected_key => Ok(()),
            Some(_) => Err(anyhow!("Invalid API key")),
            None => Err(anyhow!("Authentication required but no API key configured")),
        }
    }

    /// Check if authentication is required
    pub fn is_auth_required(&self) -> bool {
        self.config.require_auth
    }
}

/// Extract bearer token from Authorization header
pub fn extract_bearer_token(auth_header: Option<&str>) -> Option<String> {
    let auth = auth_header?;
    if !auth.starts_with("Bearer ") {
        return None;
    }
    Some(auth[7..].to_string())
}
