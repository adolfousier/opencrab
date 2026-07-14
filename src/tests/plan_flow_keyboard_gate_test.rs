//! Regression: the plan Approve/Discard keyboard must appear only once the
//! editing turn has SETTLED, never while the turn is in flight (#571).
//!
//! `/execute` and the Approve tap are both refused while a turn is running, so
//! surfacing the button mid-edit only invites a tap that bounces with "a turn
//! is running" (and the button flickers as each in-flight refresh re-attaches
//! then strips it). `plan_state_chrome` gates the keyboard on `!turn_active`
//! for the PostInitEditing state; the settle path re-runs `refresh_sections`
//! so the button materializes at turn end.

use crate::channels::telegram::flow_chrome::{PlanKb, plan_state_chrome};
use crate::utils::plan_files::PlanModeState;

#[test]
fn editing_hides_keyboard_while_turn_active() {
    let (label, kb) = plan_state_chrome(PlanModeState::PostInitEditing, true, false);
    assert_eq!(label.as_deref(), Some("✍️ Editing plan"));
    assert_eq!(
        kb,
        PlanKb::None,
        "no actionable Approve button while the editing turn is still running"
    );
}

#[test]
fn editing_shows_keyboard_at_settle() {
    let (label, kb) = plan_state_chrome(PlanModeState::PostInitEditing, false, false);
    assert_eq!(label.as_deref(), Some("✍️ Editing plan"));
    assert_eq!(
        kb,
        PlanKb::ApproveDiscard,
        "Approve/Discard appears once the turn settles, when approval is accepted"
    );
}

#[test]
fn pre_init_never_shows_keyboard() {
    // Discussing the plan (pre-init) carries no keyboard in either phase.
    assert_eq!(
        plan_state_chrome(PlanModeState::PreInitEditing, true, false).1,
        PlanKb::None
    );
    assert_eq!(
        plan_state_chrome(PlanModeState::PreInitEditing, false, false).1,
        PlanKb::None
    );
}

#[test]
fn active_seed_window_keeps_discard_only_in_both_phases() {
    // The checklist-seed window is a separate concern and unchanged: it shows
    // Discard-only whether the seed turn is running or already ended.
    let (running_label, running_kb) = plan_state_chrome(PlanModeState::Active, true, true);
    assert_eq!(running_label.as_deref(), Some("⏳ Building checklist…"));
    assert_eq!(running_kb, PlanKb::DiscardOnly);

    let (done_label, done_kb) = plan_state_chrome(PlanModeState::Active, false, true);
    assert!(
        done_label.as_deref().unwrap_or_default().contains("retry"),
        "a stalled seed surfaces the retry hint"
    );
    assert_eq!(done_kb, PlanKb::DiscardOnly);
}

#[test]
fn active_outside_seed_window_has_no_label() {
    let (label, kb) = plan_state_chrome(PlanModeState::Active, false, false);
    assert_eq!(label, None);
    assert_eq!(kb, PlanKb::DiscardOnly);
}

#[test]
fn no_plan_has_no_chrome() {
    assert_eq!(
        plan_state_chrome(PlanModeState::NoPlan, false, false),
        (None, PlanKb::None)
    );
}
