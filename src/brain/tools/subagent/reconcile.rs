//! Startup reconciliation of orphaned status files (#1038, unified #26).
//!
//! A sub-agent runs inside a `tokio::spawn`ed task, so it dies with the
//! process. Its status file does not: it stays on disk in whatever state it
//! last wrote, which for a killed agent is `Pending` or `Running` forever.
//! Anything reading those files then sees a live agent that no longer exists.
//! Detached commands share the dir and the failure mode since #26.
//!
//! Nothing reconciled them, and the sweep the module documented was never
//! wired: [`crate::brain::agent::service::work_status::cleanup_stale`] had
//! no caller outside tests, so orphaned files accumulated indefinitely.
//!
//! This pass runs once at startup, before any new work is spawned. Every
//! non-terminal file belongs to a dead process by definition, so each is
//! moved to `Interrupted` on disk. Only AGENT files are
//! returned for reporting: a command's interruption is still reported from
//! its DB row (#763), and reporting both kinds here would double-message
//! until the boot pass unifies (issue #26 P2).

use std::time::Duration;

use crate::brain::agent::service::work_status::{self, WorkKind, WorkStatus, status_dir};

/// How long a terminal status file is kept before the sweep removes it.
/// Matches the 7 days the module has always documented.
pub const STALE_AFTER: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Move every non-terminal status file to `Interrupted`, then sweep files
/// that have aged out.
///
/// Returns the AGENTS that were interrupted, oldest file first, so the
/// caller can tell their sessions. An agent whose file cannot be re-written
/// is still returned: the session deserves the report even when the file is
/// stuck, and the write failure is logged rather than swallowed. Interrupted
/// COMMAND files are marked on disk (their state must stop reading as live)
/// but not returned — the DB row scan owns their report (#763).
///
/// Ordering matters. Legacy migration runs first so pre-#26 agent files are
/// already home when reconciliation looks. Reconciliation runs before the
/// sweep so an interrupted item gets its finish stamped first and ages out
/// on the same schedule as any other terminal state, instead of lingering
/// as an unexplained file.
pub fn reconcile_orphaned_agents() -> Vec<WorkStatus> {
    let migrated = work_status::migrate_legacy_dir(&work_status::legacy_dir());
    if migrated > 0 {
        tracing::info!(
            target: "subagent",
            "Migrated {migrated} legacy sub-agent status file(s) into the unified dir"
        );
    }

    let interrupted = mark_orphans_interrupted();

    match work_status::cleanup_stale(STALE_AFTER) {
        Ok((scanned, removed)) if removed > 0 => {
            tracing::info!(
                target: "subagent",
                "Swept {removed} stale detached-work status file(s) of {scanned} scanned"
            );
        }
        Ok(_) => {}
        Err(e) => {
            // Costs disk, not correctness, so startup continues.
            tracing::warn!(target: "subagent", "Detached-work status sweep failed: {e}");
        }
    }

    interrupted
        .into_iter()
        .filter(|status| status.kind == WorkKind::Agent)
        .collect()
}

/// Walk the status directory and interrupt everything still mid-flight.
fn mark_orphans_interrupted() -> Vec<WorkStatus> {
    let dir = status_dir();
    if !dir.exists() {
        return Vec::new();
    }

    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!(
                target: "subagent",
                "Could not read detached-work status dir {}: {e}",
                dir.display()
            );
            return Vec::new();
        }
    };

    let mut orphans: Vec<WorkStatus> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        // A file we cannot read or parse is not silently skipped: it may be
        // stranded work, and going quiet here is how the original bug hid.
        // (Pre-#26 COMMAND files carry the old schema and land here too;
        // their recovery rides the DB row, #763, so the warn is the whole
        // story for them.)
        let data = match std::fs::read_to_string(&path) {
            Ok(data) => data,
            Err(e) => {
                tracing::warn!(
                    target: "subagent",
                    "Could not read detached-work status file {}: {e}",
                    path.display()
                );
                continue;
            }
        };
        let mut status: WorkStatus = match serde_json::from_str(&data) {
            Ok(status) => status,
            Err(e) => {
                tracing::warn!(
                    target: "subagent",
                    "Could not parse detached-work status file {}: {e}",
                    path.display()
                );
                continue;
            }
        };
        if status.state.is_terminal() {
            continue;
        }
        if let Err(e) = status.mark_interrupted() {
            tracing::error!(
                target: "subagent",
                "Detached work {} was interrupted by a restart but its status could not be \
                 updated, so it will keep reading as running: {e}",
                status.id
            );
        }
        match status.kind {
            WorkKind::Agent => tracing::warn!(
                target: "subagent",
                "Sub-agent '{}' ({}) for session {} was interrupted by a restart",
                status.label,
                status.id,
                status.session_id
            ),
            WorkKind::Command => tracing::warn!(
                target: "background_task",
                "Detached command '{}' ({}) for session {} was interrupted by a restart \
                 (report rides the DB row, #763)",
                status.label,
                status.id,
                status.session_id
            ),
        }
        orphans.push(status);
    }

    orphans.sort_by(|a, b| a.spawned_at.cmp(&b.spawned_at));
    orphans
}
