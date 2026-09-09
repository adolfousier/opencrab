//! Whether this process is on its way down (#1462).
//!
//! A turn that ends carrying `AgentError::Cancelled` can mean two opposite
//! things, and the recovery ticket must be handled differently for each:
//!
//! * **The user stopped it** — Esc twice, `/stop`, `/discard`, a stop word.
//!   They abandoned the work, so the tracking row is deleted and the turn is
//!   never replayed.
//! * **The app is quitting under it** — Ctrl+C twice cancels the in-flight
//!   token before setting `should_quit` (`tui/app/state.rs`). The user asked
//!   for the process to end, not for the turn to be thrown away, so the row
//!   must survive for the next boot to resume.
//!
//! `CancellationToken` carries no reason, so the shutdown paths raise this
//! flag before cancelling and the tool loop reads it when deciding whether to
//! delete the row. Process-global on purpose: it describes the process, and
//! the reader is several layers below the TUI that sets it.
//!
//! Not needed for `/restart`, `/exit`, `/quit` or `TuiEvent::Quit`: those end
//! the process without cancelling, so the row already survives. They set the
//! flag anyway, so the meaning stays "this process is going down" rather than
//! "one particular key combination was pressed".

use std::sync::atomic::{AtomicBool, Ordering};

static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

/// Mark the process as shutting down. Called before any shutdown path
/// cancels an in-flight turn.
pub(crate) fn mark_shutting_down() {
    SHUTTING_DOWN.store(true, Ordering::SeqCst);
}

/// True once a shutdown has begun.
pub(crate) fn is_shutting_down() -> bool {
    SHUTTING_DOWN.load(Ordering::SeqCst)
}

/// Should this turn's recovery row survive?
///
/// Generic over the success type so the decision can be exercised directly
/// without building an `AgentResponse`. Only a cancellation that coincides
/// with a shutdown keeps the row: a user-initiated stop still deletes it, and
/// a turn that merely *finished* during a shutdown deletes it too, or the next
/// boot would replay work that was already answered.
pub(crate) fn keeps_recovery_row<T>(
    result: &Result<T, crate::brain::agent::error::AgentError>,
) -> bool {
    matches!(
        result,
        Err(crate::brain::agent::error::AgentError::Cancelled)
    ) && is_shutting_down()
}

/// Test-only reset so cases cannot leak the flag into each other.
#[cfg(test)]
pub(crate) fn reset_for_test() {
    SHUTTING_DOWN.store(false, Ordering::SeqCst);
}
