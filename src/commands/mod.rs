//! Command handlers

pub mod setup;
pub mod chat;
pub mod config;
pub mod skill;
pub mod provider;
pub mod persona;
pub mod status;
pub mod run;
pub mod stop;

pub use setup::handle_setup;
pub use chat::handle_chat;
pub use config::handle_config;
pub use skill::handle_skill;
pub use provider::handle_provider;
pub use persona::handle_persona;
pub use status::handle_status;
pub use run::handle_run;
pub use stop::handle_stop;