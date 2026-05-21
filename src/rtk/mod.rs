//! RTK (Rust Token Killer) integration module
//!
//! This module provides token-saving functionality by filtering and compressing
//! bash command outputs before they reach the LLM context. It wraps the `rtk`
//! CLI binary to achieve 60-90% token savings on common development commands.
//!
//! # Features
//! - Command rewriting via `rtk rewrite`
//! - Stats extraction (git diff, cargo build, etc.)
//! - Error-only mode for test runs
//! - Output grouping and deduplication
//! - Structure-only mode for JSON/data files
//!
//! # Usage
//! Enable with the `rtk` feature flag:
//! ```bash
//! cargo run --features rtk
//! ```

#[cfg(feature = "rtk")]
mod rewrite;
#[cfg(feature = "rtk")]
mod tracker;

#[cfg(feature = "rtk")]
pub use rewrite::{RtkResult, is_rtk_available, rewrite_command};
#[cfg(feature = "rtk")]
pub use tracker::{RtkMetrics, RtkTracker, TokenSavings, global_tracker};

#[cfg(not(feature = "rtk"))]
pub fn is_rtk_available() -> bool {
    false
}

#[cfg(not(feature = "rtk"))]
pub fn rewrite_command(_command: &str) -> Option<String> {
    None
}
