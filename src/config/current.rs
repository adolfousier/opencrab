//! Process-wide current-config mirror.
//!
//! config.toml is the source of truth — but reading, parsing, and key-merging
//! it from disk on every request (it was hit ~50× per turn) is pure waste. This
//! keeps a single in-memory copy that the config watcher refreshes the instant
//! the file changes. Reads are then a cheap `Arc` clone with zero disk IO, and
//! the mirror is never stale and never a drifting shadow: the file stays
//! authoritative and the watcher is the only writer.
//!
//! Use [`Config::current`] everywhere a request/turn needs config. Reserve the
//! disk-reading `Config::load` for exactly three places: startup seeding, the
//! watcher itself, and write-then-reread.

use super::types::Config;
use std::sync::{Arc, OnceLock, RwLock};

static CURRENT: OnceLock<RwLock<Arc<Config>>> = OnceLock::new();

/// The mirror cell, seeded from disk on first touch if startup hasn't already
/// called [`Config::set_current`]. Seeding from disk here is the only place
/// `current()` ever does IO, and only once.
fn cell() -> &'static RwLock<Arc<Config>> {
    CURRENT.get_or_init(|| {
        let cfg = Config::load().unwrap_or_else(|e| {
            tracing::warn!(
                "current-config: initial disk load failed ({e}); using embedded defaults"
            );
            embedded_default()
        });
        RwLock::new(Arc::new(cfg))
    })
}

/// A valid `Config` parsed from the embedded `config.toml.example`. Used only
/// if the very first `current()` happens before startup seeding AND the disk
/// load fails (fresh install pre-onboarding) — `Config` has no `Default`.
fn embedded_default() -> Config {
    toml::from_str(include_str!("../../config.toml.example"))
        .expect("embedded config.toml.example must parse")
}

impl Config {
    /// The current config: a cheap `Arc` clone of the in-memory mirror the
    /// watcher refreshes on file change. Zero disk IO on the hot path.
    pub fn current() -> Arc<Config> {
        cell().read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Replace the in-memory mirror. Called once at startup (after the initial
    /// load) and by the config watcher whenever config.toml changes and parses
    /// cleanly.
    pub fn set_current(config: Config) {
        *cell().write().unwrap_or_else(|e| e.into_inner()) = Arc::new(config);
    }
}
