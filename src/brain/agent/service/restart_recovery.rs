//! Delivery of restart-recovery reports to the session that owns them (#1037).
//!
//! A report about work a previous process was doing must reach the surface
//! that started that work, not whichever surface happened to boot. Two things
//! stand in the way:
//!
//! - Recovery runs during startup, before any channel has called
//!   [`super::session_routes::register_session_route`], so the route map is
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

use super::types::{MessageEnqueueCallback, PushOrigin, QueuedUserMessage};

/// How long a parked report waits for its channel to register a route before
/// it is delivered locally instead. Long enough for channels to finish
/// connecting, short enough that a report is not lost to a user who is
/// looking at the local surface right now.
pub const ROUTE_GRACE: Duration = Duration::from_secs(30);

/// Reports whose session had no route when they were produced.
static PARKED: Mutex<Vec<(Uuid, QueuedUserMessage)>> = Mutex::new(Vec::new());

/// Sessions known to belong to a channel that has not claimed them yet.
///
/// Startup crash-recovery revives a session straight into a turn, bypassing
/// the ingress handlers where `claim_for_channel` lives, so the route map
/// says nothing about it while it runs. The fallbacks then treat it as
/// local: on a daemon that is a void, and under the TUI it is worse than a
/// void, because the answer is delivered to the wrong surface and looks
/// fine. Recovery knows which channel the session came from, so it records
/// that here and the fallbacks park instead of guessing (#1206).
static AWAITING_CHANNEL: Mutex<Option<std::collections::HashSet<Uuid>>> = Mutex::new(None);

/// Record that `session_id` belongs to a channel which has not claimed it.
///
/// Idempotent. Cleared by [`claim_session`] once the owning channel binds a
/// route, so this never outlives the gap it describes.
pub fn expect_channel_route(session_id: Uuid) {
    match AWAITING_CHANNEL.lock() {
        Ok(mut guard) => {
            guard
                .get_or_insert_with(std::collections::HashSet::new)
                .insert(session_id);
        }
        Err(e) => {
            // Without the mark this session's completions fall back to the
            // local surface, which is the mis-delivery this prevents.
            tracing::error!(
                target: "background_task",
                "Could not mark session {session_id} as channel-owned: {e}"
            );
        }
    }
}

/// Does `session_id` belong to a channel that has not claimed it yet?
///
/// While true, a completion for it must be parked rather than handed to
/// whichever surface happens to be executing.
pub fn awaits_channel_route(session_id: Uuid) -> bool {
    match AWAITING_CHANNEL.lock() {
        Ok(guard) => guard.as_ref().is_some_and(|s| s.contains(&session_id)),
        Err(e) => {
            tracing::error!(
                target: "background_task",
                "Could not read the channel-owned mark for session {session_id}: {e}"
            );
            false
        }
    }
}

/// Forget the channel-owned mark for `session_id`.
fn clear_channel_expectation(session_id: Uuid) {
    match AWAITING_CHANNEL.lock() {
        Ok(mut guard) => {
            if let Some(set) = guard.as_mut() {
                set.remove(&session_id);
            }
        }
        Err(e) => {
            // Only costs a redundant park on a session that now has a real
            // route, which the route itself takes precedence over.
            tracing::warn!(
                target: "background_task",
                "Could not clear the channel-owned mark for session {session_id}: {e}"
            );
        }
    }
}

/// Deliver `msg` to whoever owns `session_id`, parking it if nobody does yet.
///
/// Returns whether it went out immediately. A parked report is not lost: it
/// leaves on the next [`claim_session`] for that session, or on
/// [`flush_parked`] when the grace period ends.
pub fn deliver_or_park(session_id: Uuid, msg: QueuedUserMessage) -> bool {
    if let Some(route) = super::session_routes::session_route(session_id) {
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

/// Park `msg` without consulting the route table at all (#21).
///
/// The caller already knows the session's own surface cannot take the
/// message right now — that is what parking means. Going back through
/// [`deliver_or_park`] would re-consult the very route whose surface just
/// refused the message, and if the refusal is stable (insert-only routes,
/// channel owned by a successor) the two functions bounce the message in a
/// zero-sleep hot loop: two WARN lines per cycle, ~80% of a core and
/// ~13 MB/s of log observed live on 2026-08-28, re-arming after every
/// restart until the guard that caused it was deleted (#19).
///
/// A message parked here leaves only on a fresh [`claim_session`] for that
/// session — re-run on every inbound message, so a live session still
/// drains promptly — or on [`flush_parked`] when the grace period ends.
pub fn park_unconditional(session_id: Uuid, msg: QueuedUserMessage) {
    match PARKED.lock() {
        Ok(mut parked) => parked.push((session_id, msg)),
        Err(e) => {
            // Same shape as the park arm of deliver_or_park: the message is
            // about to vanish, which is the exact failure this module
            // exists to prevent.
            tracing::error!(
                target: "background_task",
                "Could not park message for session {session_id}, it is lost: {e}"
            );
        }
    }
}

/// Hand a newly routed session everything parked for it.
///
/// Called when a surface registers a route, so a channel that connects after
/// startup still receives what its session missed. Returns how many went out.
pub fn claim_session(session_id: Uuid, route: &MessageEnqueueCallback) -> usize {
    // The gap this mark describes is over: a real route now owns the session.
    clear_channel_expectation(session_id);
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

/// The destination for a process that has no local surface of its own (#1206).
///
/// A headless daemon's only surfaces are its channels. It still builds the
/// TUI's enqueue callback and hands it out as the fallback for a session no
/// channel has claimed, so a completion for such a session was pushed into a
/// `TuiEvent` channel that nothing in a daemon ever drains, and the send
/// error was discarded. Nothing resumed, nothing logged.
///
/// Sessions revived by startup crash-recovery are exactly that case: they run
/// a turn without ever passing through an ingress handler, and
/// `claim_for_channel` is only reached from one, so they stay unclaimed until
/// their channel sees the next inbound message.
///
/// Parking keeps them until a channel claims the session, at which point
/// [`claim_session`] flushes them to the surface that actually owns it. This
/// is a fallback destination, never a route: a session a channel HAS claimed
/// still takes the fast path and never reaches here.
pub fn parking_route() -> MessageEnqueueCallback {
    std::sync::Arc::new(|session_id, msg: QueuedUserMessage| {
        // Worth one line each time: a parked message is not lost, but it is
        // also not delivered yet, and that difference is invisible otherwise.
        tracing::warn!(
            target: "background_task",
            "No surface claims session {session_id} and this process has no local one; \
             parking until its channel claims it: {}",
            msg.display_text
        );
        deliver_or_park(session_id, msg);
    })
}

#[cfg(test)]
pub(crate) fn clear_parked_for_test() {
    if let Ok(mut parked) = PARKED.lock() {
        parked.clear();
    }
    if let Ok(mut awaiting) = AWAITING_CHANNEL.lock()
        && let Some(set) = awaiting.as_mut()
    {
        set.clear();
    }
}

/// Serialize tests that touch the process-global parked queue or the
/// channel-owned set, and start each from a clean slate.
///
/// The lock lives beside the state it guards rather than in one suite,
/// because more than one suite exercises that state: a second suite with its
/// own lock does not serialize against the first, and the two interleave
/// their parks. Found exactly that way (#1206).
#[cfg(test)]
pub(crate) fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    static TEST_LOCK: Mutex<()> = Mutex::new(());
    let guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_parked_for_test();
    guard
}

/// Account for background tasks that were running when a previous process
/// died, then clear them.
///
/// Every surviving row belonged to a process that no longer exists, so its
/// child is gone too: there is nothing to reattach to and no result coming.
/// Each one is reported into its session as an interruption so the agent can
/// decide whether to re-run it, rather than waiting forever on a resume that
/// can never arrive (#763). Returns how many were reported.
pub async fn report_interrupted() -> usize {
    let Some(repo) = super::background_tasks::task_repo() else {
        return 0;
    };
    let rows = match repo.all().await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!(target: "background_task", "Failed to read background tasks: {e:#}");
            return 0;
        }
    };
    if rows.is_empty() {
        return 0;
    }
    let mut count = 0usize;
    for row in rows {
        tracing::warn!(
            target: "background_task",
            "Background task '{}' for session {} was interrupted by a restart",
            row.label,
            row.session_id
        );
        // By session, never by whoever booted. This path used to take the
        // caller's callback directly, so a channel session's interruption
        // landed on the local surface — the shape #940 fixed for completions
        // and left standing here. Startup runs before channels register, so
        // an unclaimed session parks rather than mis-delivers (#1037).
        deliver_or_park(row.session_id, interrupted_message(&row));
        count += 1;

        // Clear per row, only after it is accounted for. clear_all() used to
        // run regardless, so a row whose report never got produced was
        // dropped from the table anyway and its session never heard anything.
        if let Err(e) = repo.clear(row.id).await {
            // A surviving row re-reports the same interruption next start,
            // which is noisy but recoverable; the report itself already
            // landed, so this is not fatal.
            tracing::error!(
                target: "background_task",
                "Failed to clear background task '{}' after reporting it: {e:#}",
                row.label
            );
        }
    }
    count
}

/// What the agent is told about a command a restart killed. Deliberately
/// states that it did NOT finish and hands the decision back, rather than
/// re-running something expensive on the agent's behalf.
fn interrupted_message(row: &crate::db::BackgroundTaskRow) -> QueuedUserMessage {
    let context_text = format!(
        "[BACKGROUND TASK INTERRUPTED] `{}` was still running when OpenCrabs restarted, so it \
         was killed and produced no result. The command was:\n\n```\n{}\n```\n\nIt did NOT \
         complete. Decide whether to run it again based on what you were doing; do not assume \
         it passed or failed.",
        row.label, row.command
    );
    QueuedUserMessage {
        context_text,
        display_text: format!("⚠️ Background task interrupted by restart: {}", row.label),
        origin: PushOrigin::Recovery,
        bg_meta: None,
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
/// destination once the grace period expires. `None` when the process has no
/// local surface at all (a headless daemon, whose only surfaces are its
/// channels): there is nothing to flush TO, so parked reports keep waiting
/// for a channel to claim their session instead of being handed to a
/// destination that would discard them (#1206).
pub async fn recover(local: Option<MessageEnqueueCallback>) -> usize {
    // Sub-agents first: they die with the process but their status files do
    // not, so every file still mid-flight is an agent that no longer exists.
    let orphans = crate::brain::tools::subagent::reconcile::reconcile_orphaned_agents();
    let mut reported = 0usize;
    for orphan in orphans {
        match Uuid::parse_str(&orphan.session_id) {
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
                    orphan.session_id
                );
            }
        }
    }

    // Then detached commands, which keep their own table.
    reported += report_interrupted().await;

    if reported > 0 {
        tracing::info!(
            target: "background_task",
            "Recovered {reported} interrupted item(s) from a previous run, {} waiting for a \
             session route",
            parked_count()
        );
    }
    match local {
        Some(local) => schedule_flush(local),
        None => tracing::info!(
            target: "background_task",
            "No local surface to flush to; parked reports wait for a channel to claim their \
             session"
        ),
    }
    reported
}

/// What the parent agent is told about a sub-agent a restart killed.
///
/// Mirrors the framing used for detached commands: state plainly that it did
/// not finish and hand the decision back, rather than letting the agent read
/// an absent result as either success or failure.
fn subagent_interrupted_message(
    status: &crate::brain::agent::service::work_status::WorkStatus,
) -> QueuedUserMessage {
    let context_text = format!(
        "[SUB-AGENT INTERRUPTED] The sub-agent `{}` (id {}) was still running when OpenCrabs \
         restarted, so it was killed and produced no result. Its task was:\n\n```\n{}\n```\n\nIt \
         did NOT complete. Decide whether to spawn it again based on what you were doing; do not \
         assume it succeeded or failed.",
        status.label, status.id, status.task
    );
    QueuedUserMessage {
        context_text,
        display_text: format!("⚠️ Sub-agent interrupted by restart: {}", status.label),
        origin: PushOrigin::Recovery,
        bg_meta: None,
    }
}
