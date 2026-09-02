//! Per-language phantom-detection data loaded from TOML at compile time.
//!
//! Each `.toml` file defines the phrases, verbs, and regex patterns
//! for one language. The loader embeds them into the binary via
//! `include_str!` so any TOML syntax error fails the build.
//!
//! Runtime language detection picks the right config based on
//! character-set heuristics (Cyrillic → ru, etc.).
//!
//! [`config`] holds the struct and the embedded tables, [`detect`] the
//! heuristics. This file is declarations only — no function definitions
//! live here (CONTRIBUTING.md).

pub(crate) mod config;
mod detect;

pub use config::LangConfig;
pub use detect::{all_langs, detect_language};
