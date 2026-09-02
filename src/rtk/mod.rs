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
//!
//! This file is declarations only — no function definitions live here
//! (CONTRIBUTING.md); the feature-off stubs live in [`disabled`].

#[cfg(not(feature = "rtk"))]
mod disabled;
#[cfg(feature = "rtk")]
pub(crate) mod rewrite;
#[cfg(feature = "rtk")]
mod tracker;

#[cfg(not(feature = "rtk"))]
pub use disabled::{is_rtk_available, rewrite_command, warm_up};
#[cfg(feature = "rtk")]
pub use rewrite::{RTK_NOT_INSTALLED_HELP, RtkResult, is_rtk_available, rewrite_command, warm_up};
#[cfg(feature = "rtk")]
pub use tracker::{RtkMetrics, RtkTracker, TokenSavings, global_tracker};
