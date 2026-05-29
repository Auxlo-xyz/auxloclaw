//! Command handlers

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

pub use capabilities::handle_capabilities;
pub use chat::handle_chat;
pub use config::handle_config;
pub use logs::handle_logs;
pub use model::handle_model;
pub use persona::handle_persona;
pub use plan::{handle_plan, handle_run_plan};
pub use provider::handle_provider;
pub use run::handle_run;
pub use runs::handle_runs;
pub use schedule::handle_schedule;
pub use setup::handle_setup;
pub use skill::handle_skill;
pub use status::handle_status;
pub use stop::handle_stop;
pub use mcp::handle_mcp;
