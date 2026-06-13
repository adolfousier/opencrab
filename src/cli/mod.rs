//! CLI Module
//!
//! Command-line interface for OpenCrabs using Clap v4.

mod args;
pub(crate) mod commands;
pub(crate) mod crash_recovery;
mod cron;
pub(crate) mod daemon_health;
mod ui;

pub use args::*;
