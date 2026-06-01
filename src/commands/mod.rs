//! Command handlers
//!
//! Each command is exposed as `commands::<module>::<fn>`. The re-exports that
//! used to live here were never used (`main.rs` and tests always use the
//! full module path), so they've been removed.

pub mod capabilities;
pub mod chat;
pub mod code;
pub mod config;
pub mod logs;
pub mod model;
pub mod persona;
pub mod plan;
pub mod provider;
pub mod run;
pub mod runs;
pub mod schedule;
pub mod setup;
pub mod skill;
pub mod status;
pub mod stop;
pub mod update;
pub mod mcp;
pub mod token;
