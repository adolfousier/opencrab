//! Cancelling a turn must stop the turn *and* the indicator that says one is
//! running (#1342, #1343).
//!
//! Reported from a split-pane TUI: Escape twice, the operation stops, and the
//! pane keeps showing "is thinking" forever. Single pane was unaffected, which
//! is the discriminator: `panes.rs` renders the per-session sidecar, and
//! `abort_active_turn` cleared only the foreground fields, so a cancelled
//! session's live state stayed parked in `background_sessions` where nothing
//! would ever clear it.
//!
//! These pin the pieces that can be checked without a live terminal: the
//! liveness predicate the guards key on, and the sidecar contract that decides
//! whether a dead turn can be snapshotted at all.

use crate::tui::app::background_session::BackgroundSessionState;

/// The sidecar entry a cancelled turn used to leave behind: streaming text
/// and reasoning, both of which `panes.rs` draws as a live turn.
fn live_sidecar() -> BackgroundSessionState {
    BackgroundSessionState {
        streaming_reasoning: Some("weighing the options".to_string()),
        ..Default::default()
    }
}

#[test]
fn a_sidecar_holding_reasoning_reports_live_state() {
    assert!(
        live_sidecar().has_live_state(),
        "reasoning alone is what the pane renders as 'is thinking', so it must \
         count as live state"
    );
}

/// `demote_to_background` inserts only when `has_live_state()`. That guard
/// alone is not enough for a cancelled turn, whose fields can still be
/// populated at the moment it is demoted, which is why the insert is now also
/// gated on the session still processing.
#[test]
fn live_state_alone_does_not_prove_the_turn_is_running() {
    let bg = live_sidecar();
    assert!(
        bg.has_live_state(),
        "the pre-existing guard passes for a cancelled turn's leftovers, so it \
         cannot be the only thing standing between a dead turn and the sidecar"
    );
}

#[test]
fn an_empty_sidecar_is_never_live() {
    assert!(
        !BackgroundSessionState::default().has_live_state(),
        "a cleared sidecar must not render as a running turn"
    );
}

/// Streaming response and reasoning are independent surfaces: the spinner
/// keys on one, the thinking line on the other. Either alone keeps the pane
/// looking busy, so clearing only one would leave half the bug.
#[test]
fn either_streaming_surface_alone_keeps_the_pane_looking_busy() {
    let only_response = BackgroundSessionState {
        streaming_response: Some("partial answer".to_string()),
        ..Default::default()
    };
    let only_reasoning = BackgroundSessionState {
        streaming_reasoning: Some("thinking out loud".to_string()),
        ..Default::default()
    };
    assert!(only_response.has_live_state());
    assert!(only_reasoning.has_live_state());
}
