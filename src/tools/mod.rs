//! Tools module

pub mod builtin;
pub mod web;

use crate::orchestrator::{Tool, ToolDefinition};

// Re-export built-in tools
pub use builtin::*;