//! Telegram Bot Integration
//!
//! Runs a Telegram bot alongside the TUI, forwarding messages from
//! allowlisted users to the AgentService and replying with responses.

mod agent;
pub(crate) mod cowork;
pub(crate) mod dedup_approval;
pub(crate) mod delivery;
pub(crate) mod ephemeral;
pub(crate) mod flow;
pub(crate) mod flow_chrome;
pub(crate) mod follow_up_question;
pub(crate) mod group_name;
pub(crate) mod handler;
pub(crate) mod intermediates;
pub(crate) mod keyboards;
pub(crate) mod markdown;
pub(crate) mod media;
pub(crate) mod member_events;
pub(crate) mod menu_scope;
pub(crate) mod outbound_dedup;
pub(crate) mod plan_card;
pub(crate) mod rate_limit;
pub(crate) mod raw_updates;
pub(crate) mod reaction_prompt;
pub(crate) mod resume;
pub(crate) mod rich;
pub(crate) mod rich_decode;
pub(crate) mod send;
pub(crate) mod session_resolve;
pub(crate) mod suggest_followups;
pub(crate) mod telemetry;
pub(crate) mod typing;

pub use agent::TelegramAgent;
pub(crate) use agent::register_bot_commands;
#[cfg(test)]
pub(crate) use agent::{sanitize_command_name, truncate_description};

pub(crate) mod state;
pub use state::*;
