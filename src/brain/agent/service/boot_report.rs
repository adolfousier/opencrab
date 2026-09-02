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
    /// Of those, how many were user-initiated turns vs system-initiated
    /// (fork inventory: #12 pending-origin).
    interrupted_user: usize,
    interrupted_system: usize,
    /// Boot reports that landed in the PARKED queue because no surface had
    /// claimed their session yet (fork inventory: the fork's boot_parked_tg).
    parked: usize,
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

/// The same, split by who owned the turn (fork inventory: #12 pending-origin
/// rows carry "user" or "system"). Anything that is not "system" counts as
/// user, so legacy rows with an empty origin still land on the user side.
/// The split only counts a session once, mirroring the deduped `interrupted`
/// total: duplicate rows for one session are one dead turn, not two.
pub fn record_interrupted_origin(session_id: Uuid, origin: &str) {
    with(|l| {
        if l.interrupted.insert(session_id) {
            if origin == "system" {
                l.interrupted_system += 1;
            } else {
                l.interrupted_user += 1;
            }
        }
    });
}

/// A boot report went to the PARKED queue because no surface had claimed its
/// session yet. Counted only from the boot recovery paths: this ledger
/// describes the boot, not every park the process ever does.
pub fn record_parked() {
    with(|l| l.parked += 1);
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
    let (interrupted, user, system, resumed, delivered, failed, parked) = with(|l| {
        (
            l.interrupted.len(),
            l.interrupted_user,
            l.interrupted_system,
            l.resumed.iter().map(Uuid::to_string).collect::<Vec<_>>(),
            l.delivered,
            l.failed,
            l.parked,
        )
    })
    .unwrap_or_default();
    format!(
        "[boot] interrupted={interrupted} user={user} system={system} resumed=[{}] \
delivered={delivered} failed={failed} parked={parked}",
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialize tests against the process-global ledger, starting each from
    /// a clean slate — the same lesson restart_recovery learned the hard way
    /// (#1206): a second suite with its own lock does not serialize against
    /// the first.
    fn guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        let g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_for_test();
        g
    }

    fn uuid(n: u8) -> Uuid {
        Uuid::from_u64_pair(n as u64, 0)
    }

    #[test]
    fn an_empty_boot_still_says_so() {
        let _g = guard();
        assert_eq!(
            summary_line(),
            "[boot] interrupted=0 user=0 system=0 resumed=[] delivered=0 failed=0 parked=0"
        );
    }

    #[test]
    fn the_split_counts_sessions_and_legacy_rows_land_on_user() {
        let _g = guard();
        record_interrupted_origin(uuid(1), "user");
        record_interrupted_origin(uuid(2), "system");
        record_interrupted_origin(uuid(3), "");
        // A duplicate row for session 1 must not double-count the split.
        record_interrupted_origin(uuid(1), "user");
        record_parked();
        let line = summary_line();
        assert!(line.contains("interrupted=3"), "line was: {line}");
        assert!(line.contains("user=2"), "line was: {line}");
        assert!(line.contains("system=1"), "line was: {line}");
        assert!(line.contains("parked=1"), "line was: {line}");
    }

    #[test]
    fn duplicate_rows_count_as_one_session_but_every_resume_counts() {
        let _g = guard();
        // Two rows for session 1 (a re-queued turn), one for session 2.
        record_interrupted(uuid(1));
        record_interrupted(uuid(1));
        record_interrupted(uuid(2));
        record_resumed(uuid(1));
        record_resumed(uuid(2));
        record_delivered();
        record_failed();
        let line = summary_line();
        // Sessions, not rows: the ids are in full precisely so this line is
        // the whole investigation.
        assert!(line.contains("interrupted=2"), "line was: {line}");
        assert!(line.contains(&uuid(1).to_string()), "line was: {line}");
        assert!(line.contains(&uuid(2).to_string()), "line was: {line}");
        assert!(line.contains("delivered=1"), "line was: {line}");
        assert!(line.contains("failed=1"), "line was: {line}");
    }

    #[test]
    fn a_wake_that_never_left_survives_as_failed() {
        let _g = guard();
        record_interrupted(uuid(7));
        record_resumed(uuid(7));
        // The dispatch happened, the transport never came up.
        record_failed();
        let line = summary_line();
        assert!(line.contains("interrupted=1"), "line was: {line}");
        assert!(line.contains(&uuid(7).to_string()), "line was: {line}");
        assert!(line.contains("delivered=0"), "line was: {line}");
        assert!(line.contains("failed=1"), "line was: {line}");
    }
}
