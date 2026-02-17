//! WhatsApp Integration
//!
//! Runs a WhatsApp Web client alongside the TUI, forwarding messages from
//! allowlisted phone numbers to the AgentService and replying with responses.

mod agent;
mod handler;
pub(crate) mod sqlx_store;

pub use agent::WhatsAppAgent;
