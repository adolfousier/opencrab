//! Regression for #698: double-escape BEFORE the agent replies must pull the
//! query back into the input and drop it from the transcript, not leave it
//! lingering (which duplicates on resend). When the agent produced any output,
//! everything on screen survives the cancel.

use crate::tui::app::input::should_restore_cancelled_query;

#[test]
fn restores_only_when_agent_produced_nothing() {
    // Nothing produced (cancelled before any reply) -> restore the query.
    assert!(should_restore_cancelled_query(false, false, false, false));
}

#[test]
fn keeps_transcript_when_anything_was_produced() {
    // Any produced output -> keep everything on screen, do not restore.
    assert!(!should_restore_cancelled_query(true, false, false, false)); // visible text
    assert!(!should_restore_cancelled_query(false, true, false, false)); // reasoning
    assert!(!should_restore_cancelled_query(false, false, true, false)); // tool activity
    assert!(!should_restore_cancelled_query(false, false, false, true)); // intermediate
    assert!(!should_restore_cancelled_query(true, true, true, true));
}
