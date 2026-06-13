//! Tests for the Mission Control keyboard handler.
//!
//! Drives the pure `decide` fn against an `McState` directly — no need
//! for a full `App`. Covers the three layers: detail-popup-open
//! key handling, panel focus cycling, and selection navigation.

use crate::tui::app::mission_control::McPanel;
use crate::tui::app::mission_control::McState;
use crate::tui::app::mission_control::input::{KeyOutcome, decide};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::empty())
}

// ── Esc semantics ───────────────────────────────────────────────────────────

#[test]
fn esc_without_popup_returns_close() {
    let mut s = McState::default();
    let out = decide(&mut s, 5, key(KeyCode::Esc));
    assert_eq!(out, KeyOutcome::Close);
}

#[test]
fn esc_with_popup_open_just_closes_popup() {
    let mut s = McState {
        detail_open: true,
        ..Default::default()
    };
    let out = decide(&mut s, 5, key(KeyCode::Esc));
    assert_eq!(out, KeyOutcome::Consumed);
    assert!(!s.detail_open, "popup should be closed");
}

// ── Focus cycling ───────────────────────────────────────────────────────────

#[test]
fn tab_cycles_focus_forward_through_four_panels() {
    let mut s = McState::default();
    assert_eq!(s.focused_panel, McPanel::Inbox);
    decide(&mut s, 0, key(KeyCode::Tab));
    assert_eq!(s.focused_panel, McPanel::Analytics);
    decide(&mut s, 0, key(KeyCode::Tab));
    assert_eq!(s.focused_panel, McPanel::Activity);
    decide(&mut s, 0, key(KeyCode::Tab));
    assert_eq!(s.focused_panel, McPanel::Schedule);
    decide(&mut s, 0, key(KeyCode::Tab));
    assert_eq!(s.focused_panel, McPanel::Inbox, "tab should wrap");
}

#[test]
fn back_tab_cycles_focus_backward() {
    let mut s = McState::default();
    decide(&mut s, 0, key(KeyCode::BackTab));
    assert_eq!(s.focused_panel, McPanel::Schedule);
    decide(&mut s, 0, key(KeyCode::BackTab));
    assert_eq!(s.focused_panel, McPanel::Activity);
    decide(&mut s, 0, key(KeyCode::BackTab));
    assert_eq!(s.focused_panel, McPanel::Analytics);
    decide(&mut s, 0, key(KeyCode::BackTab));
    assert_eq!(s.focused_panel, McPanel::Inbox);
}

#[test]
fn h_and_l_are_vim_aliases_for_focus_navigation() {
    let mut s = McState::default();
    decide(&mut s, 0, key(KeyCode::Char('l')));
    assert_eq!(s.focused_panel, McPanel::Analytics);
    decide(&mut s, 0, key(KeyCode::Char('h')));
    assert_eq!(s.focused_panel, McPanel::Inbox);
}

#[test]
fn focus_change_resets_selection_to_zero() {
    let mut s = McState {
        selected_index: 7,
        ..Default::default()
    };
    decide(&mut s, 10, key(KeyCode::Tab));
    assert_eq!(s.selected_index, 0);
}

// ── Selection movement ──────────────────────────────────────────────────────

#[test]
fn down_increments_within_bounds() {
    let mut s = McState::default();
    decide(&mut s, 5, key(KeyCode::Down));
    assert_eq!(s.selected_index, 1);
    decide(&mut s, 5, key(KeyCode::Char('j')));
    assert_eq!(s.selected_index, 2);
}

#[test]
fn up_decrements_clamping_at_zero() {
    let mut s = McState {
        selected_index: 1,
        ..Default::default()
    };
    decide(&mut s, 5, key(KeyCode::Up));
    assert_eq!(s.selected_index, 0);
    decide(&mut s, 5, key(KeyCode::Up));
    assert_eq!(s.selected_index, 0, "should clamp at 0");
}

#[test]
fn down_clamps_at_max_index() {
    let mut s = McState {
        selected_index: 4,
        ..Default::default()
    };
    decide(&mut s, 5, key(KeyCode::Down));
    assert_eq!(s.selected_index, 4, "should clamp at count - 1");
}

#[test]
fn empty_panel_keeps_selection_at_zero() {
    let mut s = McState {
        selected_index: 99,
        ..Default::default()
    };
    decide(&mut s, 0, key(KeyCode::Down));
    assert_eq!(s.selected_index, 0);
    decide(&mut s, 0, key(KeyCode::Up));
    assert_eq!(s.selected_index, 0);
}

#[test]
fn home_jumps_to_top() {
    let mut s = McState {
        selected_index: 7,
        ..Default::default()
    };
    decide(&mut s, 10, key(KeyCode::Home));
    assert_eq!(s.selected_index, 0);
}

#[test]
fn end_jumps_to_last_item() {
    let mut s = McState::default();
    decide(&mut s, 10, key(KeyCode::End));
    assert_eq!(s.selected_index, 9);
}

#[test]
fn g_and_capital_g_are_vim_aliases_for_home_and_end() {
    let mut s = McState {
        selected_index: 5,
        ..Default::default()
    };
    decide(&mut s, 10, key(KeyCode::Char('g')));
    assert_eq!(s.selected_index, 0);
    decide(&mut s, 10, key(KeyCode::Char('G')));
    assert_eq!(s.selected_index, 9);
}

// ── Enter / detail popup ────────────────────────────────────────────────────

#[test]
fn enter_opens_detail_when_panel_has_items() {
    let mut s = McState::default();
    decide(&mut s, 3, key(KeyCode::Enter));
    assert!(s.detail_open);
}

#[test]
fn enter_does_nothing_when_panel_is_empty() {
    let mut s = McState::default();
    decide(&mut s, 0, key(KeyCode::Enter));
    assert!(
        !s.detail_open,
        "Enter on an empty panel should not open the detail popup"
    );
}

#[test]
fn navigation_works_while_popup_open() {
    // The popup mirrors selection from the underlying panel — j/k
    // should still scroll the list under the popup.
    let mut s = McState {
        detail_open: true,
        selected_index: 1,
        ..Default::default()
    };
    decide(&mut s, 5, key(KeyCode::Down));
    assert!(s.detail_open, "popup must stay open during nav");
    assert_eq!(s.selected_index, 2);
    decide(&mut s, 5, key(KeyCode::Char('k')));
    assert_eq!(s.selected_index, 1);
}

#[test]
fn unrecognised_key_is_not_consumed() {
    let mut s = McState::default();
    let out = decide(&mut s, 5, key(KeyCode::F(12)));
    assert_eq!(out, KeyOutcome::NotConsumed);
}

// ── Apply / reject ──────────────────────────────────────────────────────────

#[test]
fn a_on_inbox_with_items_returns_apply_selected() {
    let mut s = McState::default();
    assert_eq!(s.focused_panel, McPanel::Inbox);
    let out = decide(&mut s, 3, key(KeyCode::Char('a')));
    assert_eq!(out, KeyOutcome::ApplySelected);
}

#[test]
fn a_on_inbox_when_empty_does_nothing() {
    let mut s = McState::default();
    let out = decide(&mut s, 0, key(KeyCode::Char('a')));
    // Swallowed silently — no apply, no fallthrough.
    assert_eq!(out, KeyOutcome::Consumed);
}

#[test]
fn a_on_activity_panel_is_swallowed_not_applied() {
    let mut s = McState {
        focused_panel: McPanel::Activity,
        ..Default::default()
    };
    let out = decide(&mut s, 5, key(KeyCode::Char('a')));
    assert_eq!(
        out,
        KeyOutcome::Consumed,
        "apply only fires from the inbox panel"
    );
}

#[test]
fn r_on_inbox_with_items_returns_reject_selected() {
    let mut s = McState::default();
    let out = decide(&mut s, 3, key(KeyCode::Char('r')));
    assert_eq!(out, KeyOutcome::RejectSelected);
}

#[test]
fn r_on_schedule_panel_is_swallowed_not_rejected() {
    let mut s = McState {
        focused_panel: McPanel::Schedule,
        ..Default::default()
    };
    let out = decide(&mut s, 5, key(KeyCode::Char('r')));
    assert_eq!(out, KeyOutcome::Consumed);
}

#[test]
fn apply_reject_do_not_fire_while_popup_is_open() {
    // The popup-mode handler accepts only Esc + j/k. `a` and `r` are
    // not recognised there — they fall back to NotConsumed so the
    // user can't fire actions through the popup overlay.
    let mut s = McState {
        detail_open: true,
        ..Default::default()
    };
    let a_out = decide(&mut s, 5, key(KeyCode::Char('a')));
    assert_eq!(a_out, KeyOutcome::NotConsumed);
    let r_out = decide(&mut s, 5, key(KeyCode::Char('r')));
    assert_eq!(r_out, KeyOutcome::NotConsumed);
}
