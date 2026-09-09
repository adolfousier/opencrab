//! A turn cancelled by shutdown keeps its recovery row (#1462).
//!
//! Reported: a turn in flight during a manual restart was never resumed, while
//! non-CLI sessions with work in flight came back fine.
//!
//! Mechanism: quitting with Ctrl+C twice cancels the in-flight token *before*
//! setting `should_quit` (`tui/app/state.rs`). The cancelled turn then unwound
//! through the normal return path in `run_tool_loop` and deleted its own
//! `pending_requests` row, so the next boot had nothing to resume. Confirmed in
//! the logs of the reported incident:
//!
//! ```text
//! 00:05:12.960  Stream cancelled by user      (the generic message for ANY
//!                                              token fire, not proof of a
//!                                              deliberate stop)
//! 00:05:12.960  Stream aborted by cancellation (token fired during call)
//! 00:05:16.027  OpenCrabs debug logging enabled  ← the relaunch
//! 00:05:19.369  Found 1 interrupted request(s)   ← a DIFFERENT session
//! ```
//!
//! The distinction that matters is *why* the token fired. Both a deliberate
//! stop and a shutdown surface as `AgentError::Cancelled`, and
//! `CancellationToken` carries no reason, so the shutdown paths raise a flag
//! before cancelling and the decision reads it.

use crate::brain::agent::error::AgentError;
use crate::brain::agent::service::shutdown;

/// Serialises the cases that touch the flag.
///
/// The flag is process-global by design (the tool loop reads it several layers
/// below the TUI that sets it), and libtest runs these in parallel threads of
/// one process — so without this lock one case's reset lands in the middle of
/// another's assertion. Found the hard way: the shutdown case failed while the
/// fix was correct.
static FLAG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Guard so a case cannot leak the process-global flag into its neighbours.
fn with_clean_flag<T>(f: impl FnOnce() -> T) -> T {
    let _guard = FLAG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    shutdown::reset_for_test();
    let out = f();
    shutdown::reset_for_test();
    out
}

#[test]
fn a_turn_cancelled_by_shutdown_keeps_its_row() {
    with_clean_flag(|| {
        shutdown::mark_shutting_down();
        let cancelled: Result<(), AgentError> = Err(AgentError::Cancelled);
        assert!(
            shutdown::keeps_recovery_row(&cancelled),
            "the row must survive a shutdown cancellation, or the next boot has \
             nothing to resume — this is the #1462 data loss"
        );
    });
}

#[test]
fn a_user_stop_still_deletes_the_row() {
    with_clean_flag(|| {
        // No shutdown: Esc twice, /stop, /discard, a stop word.
        let cancelled: Result<(), AgentError> = Err(AgentError::Cancelled);
        assert!(
            !shutdown::keeps_recovery_row(&cancelled),
            "a deliberately stopped turn must NOT be resumed; the user abandoned it"
        );
    });
}

#[test]
fn a_turn_that_finished_during_shutdown_still_deletes_its_row() {
    with_clean_flag(|| {
        shutdown::mark_shutting_down();
        let finished: Result<(), AgentError> = Ok(());
        assert!(
            !shutdown::keeps_recovery_row(&finished),
            "a turn that completed must never keep its row, even mid-shutdown, or \
             the next boot replays work that was already answered"
        );
    });
}

#[test]
fn other_failures_during_shutdown_do_not_keep_the_row() {
    with_clean_flag(|| {
        shutdown::mark_shutting_down();
        let failed: Result<(), AgentError> = Err(AgentError::SessionNotFound(uuid::Uuid::nil()));
        assert!(
            !shutdown::keeps_recovery_row(&failed),
            "only a cancellation is resumable; other errors would replay a turn \
             that failed for its own reasons"
        );
    });
}

#[test]
fn the_flag_starts_down_and_latches() {
    with_clean_flag(|| {
        assert!(!shutdown::is_shutting_down(), "must start down");
        shutdown::mark_shutting_down();
        assert!(shutdown::is_shutting_down(), "must latch once raised");
    });
}

// ── wiring: the flag has to be raised BEFORE the cancel ──────────────────

const STATE_SRC: &str = include_str!("../tui/app/state.rs");
const TOOL_LOOP_SRC: &str = include_str!("../brain/agent/service/tool_loop.rs");

/// The unit cases above prove the decision is correct; this proves the delete
/// actually asks it. Without this, `keeps_recovery_row` could be perfect and
/// still never consulted — which is exactly what the original bug was.
#[test]
fn the_delete_is_guarded_by_the_shutdown_decision() {
    assert!(
        TOOL_LOOP_SRC
            .contains("let cancelled_by_shutdown = super::shutdown::keeps_recovery_row(&result);"),
        "the recovery-row decision must be taken from shutdown::keeps_recovery_row; \
         a hardcoded value or an inline match silently reintroduces #1462"
    );

    let block = TOOL_LOOP_SRC
        .split("let cancelled_by_shutdown =")
        .nth(1)
        .expect("the shutdown decision is gone from the cleanup path");
    let block = &block[..block
        .find("Failed to clean up pending request")
        .unwrap_or(block.len())];

    assert!(
        block.contains("&& !cancelled_by_shutdown"),
        "the delete must be skipped when the turn was cancelled by shutdown; \
         without this guard the row is destroyed and the next boot has nothing \
         to resume (#1462)"
    );
}

#[test]
fn ctrl_c_quit_raises_the_flag_before_cancelling() {
    let block = STATE_SRC
        .split("// Second Ctrl+C within window — quit")
        .nth(1)
        .expect("the Ctrl+C quit path is gone");
    let block = &block[..block.find("should_quit = true").unwrap_or(block.len())];

    let flag_at = block
        .find("mark_shutting_down()")
        .expect("the Ctrl+C quit path must raise the shutdown flag (#1462)");
    let cancel_at = block
        .find("token.cancel()")
        .expect("the Ctrl+C quit path still cancels the in-flight token");

    assert!(
        flag_at < cancel_at,
        "the flag must be raised BEFORE the cancel; raising it afterwards races \
         the unwinding turn, which may already have deleted its row"
    );
}
