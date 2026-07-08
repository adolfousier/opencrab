//! CLI Module
//!
//! Command-line interface for OpenCrabs using Clap v4.

mod args;
pub(crate) mod commands;
pub(crate) mod crash_recovery;
mod cron;
pub(crate) mod daemon_health;
pub(crate) mod migrate;
pub(crate) mod session_set_model;
pub(crate) mod tool_setup;
mod ui;

pub use args::*;
