//! Per-session working-directory restore.
//!
//! `sessions.working_directory` persists whatever `/cd` last selected, but the
//! in-memory per-session handle (#703) is seeded lazily from the *global*
//! working directory — the one the process was launched in. Nothing ever read
//! the persisted column back for a channel session, so every Telegram/Discord/
//! Slack chat silently reverted to the launch directory on restart while the
//! DB still claimed the directory the user had picked.
//!
//! This module owns the one decision that restore needs: given the persisted
//! string, is there a directory worth hydrating the handle with? Keeping it
//! pure means the tilde handling and the stale-path case are testable without
//! a database or a running agent.

use std::path::PathBuf;

/// Resolve a persisted `sessions.working_directory` into a directory to
/// restore, or `None` when there is nothing to restore.
///
/// `None` is returned when the column is unset, blank, or points at a path
/// that is no longer a directory — a repo that has since been moved or deleted
/// must not strand the session in a cwd that cannot resolve any tool call.
/// Paths are stored in `~/...` collapsed form, so the tilde is expanded here.
pub fn restorable_cwd(persisted: Option<&str>) -> Option<PathBuf> {
    let raw = persisted?.trim();
    if raw.is_empty() {
        return None;
    }
    let expanded = crate::brain::tools::error::expand_tilde(raw);
    expanded.is_dir().then_some(expanded)
}
