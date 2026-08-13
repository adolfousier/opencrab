//! Delivery of restart-recovery reports to the session that owns them (#1037).
//!
//! A report about work a previous process was doing must reach the surface
//! that started that work, not whichever surface happened to boot. Two things
//! stand in the way:
//!
//! - Recovery runs during startup, before any channel has called
//!   [`super::background_tasks::register_session_route`], so the route map is
//!   empty at the moment the report is produced. Delivering immediately sends
//!   a channel session's report to the local surface, which is the bug #940
//!   fixed for the completion path and the restart path never got.
//! - A channel that never comes up in this run would strand the report
//!   forever if we simply waited for a route.
//!
//! So a report is parked when no route claims its session yet, delivered the
//! moment one does, and flushed to the local surface once the grace period
//! expires. Nothing is dropped on either branch.

use std::sync::Mutex;
use std::time::Duration;

use uuid::Uuid;

use super::types::{MessageEnqueueCallback, QueuedUserMessage};

/// How long a parked report waits for its channel to register a route before
/// it is delivered locally instead. Long enough for channels to finish
/// connecting, short enough that a report is not lost to a user who is
/// looking at the local surface right now.
pub const ROUTE_GRACE: Duration = Duration::from_secs(30);

/// Reports whose session had no route when they were produced.
static PARKED: Mutex<Vec<(Uuid, QueuedUserMessage)>> = Mutex::new(Vec::new());

/// Deliver `msg` to whoever owns `session_id`, parking it if nobody does yet.
///
/// Returns whether it went out immediately. A parked report is not lost: it
/// leaves on the next [`claim_session`] for that session, or on
/// [`flush_parked`] when the grace period ends.
pub fn deliver_or_park(session_id: Uuid, msg: QueuedUserMessage) -> bool {
    if let Some(route) = super::background_tasks::session_route(session_id) {
        route(session_id, msg);
        return true;
    }
    match PARKED.lock() {
        Ok(mut parked) => parked.push((session_id, msg)),
        Err(e) => {
            // The report is about to vanish, which is the exact failure this
            // module exists to prevent, so it is an error rather than a warn.
            tracing::error!(
                target: "background_task",
                "Could not park restart report for session {session_id}, it is lost: {e}"
            );
        }
    }
    false
}

/// Hand a newly routed session everything parked for it.
///
/// Called when a surface registers a route, so a channel that connects after
/// startup still receives what its session missed. Returns how many went out.
pub fn claim_session(session_id: Uuid, route: &MessageEnqueueCallback) -> usize {
    let mine = match PARKED.lock() {
        Ok(mut parked) => {
            let mut mine = Vec::new();
            parked.retain(|(id, msg)| {
                if *id == session_id {
                    mine.push(msg.clone());
                    false
                } else {
                    true
                }
            });
            mine
        }
        Err(e) => {
            tracing::error!(
                target: "background_task",
                "Could not read parked restart reports for session {session_id}: {e}"
            );
            return 0;
        }
    };
    let count = mine.len();
    for msg in mine {
        route(session_id, msg);
    }
    if count > 0 {
        tracing::info!(
            target: "background_task",
            "Delivered {count} parked restart report(s) to session {session_id}"
        );
    }
    count
}

/// Deliver everything still parked to `local`, whatever session it belongs to.
///
/// The last resort: the owning channel never came up in this run, and holding
/// the report indefinitely would be the same silent loss as never producing
/// it. Returns how many were flushed.
pub fn flush_parked(local: &MessageEnqueueCallback) -> usize {
    let remaining = match PARKED.lock() {
        Ok(mut parked) => std::mem::take(&mut *parked),
        Err(e) => {
            tracing::error!(
                target: "background_task",
                "Could not flush parked restart reports: {e}"
            );
            return 0;
        }
    };
    let count = remaining.len();
    for (session_id, msg) in remaining {
        local(session_id, msg);
    }
    if count > 0 {
        tracing::info!(
            target: "background_task",
            "No route claimed {count} restart report(s) within the grace period, delivered locally"
        );
    }
    count
}

/// Schedule [`flush_parked`] to run once the grace period is over.
pub fn schedule_flush(local: MessageEnqueueCallback) {
    tokio::spawn(async move {
        tokio::time::sleep(ROUTE_GRACE).await;
        flush_parked(&local);
    });
}

/// How many reports are waiting for a route. Exposed for tests and for a
/// surface that wants to say something is still pending.
pub fn parked_count() -> usize {
    PARKED.lock().map(|p| p.len()).unwrap_or(0)
}

#[cfg(test)]
pub(crate) fn clear_parked_for_test() {
    if let Ok(mut parked) = PARKED.lock() {
        parked.clear();
    }
}

/// Account for everything a previous process was doing, and arrange for the
/// reports to reach the right sessions.
///
/// Run once per process start by every surface: the TUI, the daemon, and the
/// headless commands. It used to live only in the TUI's startup, so a daemon
/// start left interrupted work unreported and its rows in the table until
/// somebody happened to open the TUI, at which point work from an arbitrarily
/// old process was announced as if it had just died.
///
/// `local` is the surface doing the booting, used only as the last-resort
/// destination once the grace period expires.
pub async fn recover(local: MessageEnqueueCallback) -> usize {
    // Sub-agents first: they die with the process but their status files do
    // not, so every file still mid-flight is an agent that no longer exists.
    let orphans = crate::brain::tools::subagent::reconcile::reconcile_orphaned_agents();
    let mut reported = 0usize;
    for orphan in orphans {
        match Uuid::parse_str(&orphan.parent_session_id) {
            Ok(session_id) => {
                deliver_or_park(session_id, subagent_interrupted_message(&orphan));
                reported += 1;
            }
            Err(e) => {
                // Nothing to route to. Say so rather than dropping it, since
                // the agent's parent is waiting on a result either way.
                tracing::error!(
                    target: "background_task",
                    "Sub-agent '{}' has an unparseable parent session '{}', its interruption \
                     cannot be reported: {e}",
                    orphan.label,
                    orphan.parent_session_id
                );
            }
        }
    }

    // Then detached commands, which keep their own table.
    reported += super::background_tasks::report_interrupted().await;

    if reported > 0 {
        tracing::info!(
            target: "background_task",
            "Recovered {reported} interrupted item(s) from a previous run, {} waiting for a \
             session route",
            parked_count()
        );
    }
    schedule_flush(local);
    reported
}

/// What the parent agent is told about a sub-agent a restart killed.
///
/// Mirrors the framing used for detached commands: state plainly that it did
/// not finish and hand the decision back, rather than letting the agent read
/// an absent result as either success or failure.
fn subagent_interrupted_message(
    status: &crate::brain::tools::subagent::status::AgentStatus,
) -> QueuedUserMessage {
    let context_text = format!(
        "[SUB-AGENT INTERRUPTED] The sub-agent `{}` (id {}) was still running when OpenCrabs \
         restarted, so it was killed and produced no result. Its task was:\n\n```\n{}\n```\n\nIt \
         did NOT complete. Decide whether to spawn it again based on what you were doing; do not \
         assume it succeeded or failed.",
        status.label, status.id, status.prompt
    );
    QueuedUserMessage {
        context_text,
        display_text: format!("⚠️ Sub-agent interrupted by restart: {}", status.label),
    }
}
