//! One line at the end of boot saying what the restart cost and what came
//! back (#1242).
//!
//! Boot spends real effort recovering interrupted work, and until now said
//! almost nothing about how it went. The recovery pass logged counts but not
//! which sessions, and completion was logged only on the inline path, so a
//! wake that never arrived surfaced nowhere: finding the one in the issue
//! took correlating two log lines a millisecond apart across two files.
//!
//! The ledger is process-global because the thing being described is the
//! process. It is written from the boot pass and from the spawned resumes it
//! dispatches, and read once, after every bounded wait has necessarily
//! resolved.

use std::collections::BTreeSet;
use std::sync::Mutex;
use uuid::Uuid;

/// What boot found and what became of it.
#[derive(Default)]
struct Ledger {
    /// Sessions that were mid-turn when the previous process died.
    interrupted: BTreeSet<Uuid>,
    /// Sessions a resume was actually dispatched for, after dedup.
    resumed: BTreeSet<Uuid>,
    /// Resumes whose answer reached its surface.
    delivered: usize,
    /// Resumes that errored, or whose channel never connected.
    failed: usize,
}

static LEDGER: Mutex<Option<Ledger>> = Mutex::new(None);

/// Run `f` against the ledger, creating it on first touch.
///
/// A poisoned lock costs the summary line, never the resume: every caller
/// here is bookkeeping alongside work that has already happened.
fn with<R>(f: impl FnOnce(&mut Ledger) -> R) -> Option<R> {
    match LEDGER.lock() {
        Ok(mut guard) => Some(f(guard.get_or_insert_with(Ledger::default))),
        Err(e) => {
            tracing::warn!("Boot report ledger is unreadable, the summary will be short: {e}");
            None
        }
    }
}

/// A session was mid-turn when the previous process died.
pub fn record_interrupted(session_id: Uuid) {
    with(|l| l.interrupted.insert(session_id));
}

/// A resume was dispatched for `session_id`.
pub fn record_resumed(session_id: Uuid) {
    with(|l| l.resumed.insert(session_id));
}

/// A dispatched resume got its answer out.
pub fn record_delivered() {
    with(|l| l.delivered += 1);
}

/// A dispatched resume did not: the turn errored, or its channel never came
/// up inside the connect grace.
pub fn record_failed() {
    with(|l| l.failed += 1);
}

/// The summary line.
///
/// Ids in full, not counts: the whole point is being able to go from this
/// line to the session that did not come back. Emitted even when everything
/// is zero, because "this boot had nothing to recover" is the fact that
/// makes its absence meaningful on the boots that did.
pub fn summary_line() -> String {
    let (interrupted, resumed, delivered, failed) = with(|l| {
        (
            l.interrupted.len(),
            l.resumed.iter().map(Uuid::to_string).collect::<Vec<_>>(),
            l.delivered,
            l.failed,
        )
    })
    .unwrap_or_default();
    format!(
        "[boot] interrupted={interrupted} resumed=[{}] delivered={delivered} failed={failed}",
        resumed.join(" ")
    )
}

/// Emit the summary once every bounded wait has had its chance to resolve.
///
/// Deliberately later than the connect grace rather than immediately after
/// the pass: the resumes are spawned, so counting them at dispatch time
/// would report `delivered=0` on every boot and mean nothing. Waiting one
/// grace window past the last dispatch is the earliest point at which a
/// still-missing wake is genuinely missing.
pub fn schedule_summary(after: std::time::Duration) {
    tokio::spawn(async move {
        tokio::time::sleep(after).await;
        tracing::info!(target: "background_task", "{}", summary_line());
    });
}

#[cfg(test)]
pub(crate) fn reset_for_test() {
    if let Ok(mut guard) = LEDGER.lock() {
        *guard = None;
    }
}
