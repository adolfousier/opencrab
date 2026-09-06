use crossterm::event::KeyEvent;

use crate::tui::render::presets;
use crate::tui::render::theme::{self, Theme};
use crate::tui::render::theme_picker::*;
use crate::tui::render::user_themes;
use crossterm::event::{KeyCode, KeyModifiers};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn item(name: &'static str, t: Option<&'static Theme>, is_user: bool) -> ThemePickerItem {
    ThemePickerItem {
        name,
        theme: t,
        reason: t.is_none().then(|| "rejected.toml — bad hex".to_string()),
        is_user,
    }
}

/// Hand-built state: `open()` touches process-wide globals (preset scan,
/// active theme) and would race across parallel tests.
fn fixture() -> ThemePickerState {
    ThemePickerState {
        items: vec![
            item("crab-dark", Some(&theme::CRAB_DARK), false),
            item("dracula", Some(&presets::DRACULA), false),
            item("", None, true),
            item("solarized-dark", Some(&presets::SOLARIZED_DARK), false),
        ],
        selected: 0,
        origin: &theme::CRAB_DARK,
        scroll_offset: 0,
    }
}

fn expect_preview(action: PickerAction, want: &'static Theme) {
    match action {
        PickerAction::Preview(t) => assert!(std::ptr::eq(t, want), "got {:?}", t.name),
        other => panic!("expected Preview, got {other:?}"),
    }
}

fn expect_noop(action: PickerAction) {
    assert!(matches!(action, PickerAction::None), "got {action:?}");
}

fn expect_cancel(action: PickerAction) {
    assert!(matches!(action, PickerAction::Cancel), "got {action:?}");
}

#[test]
fn down_previews_next_valid_row() {
    let mut s = fixture();
    expect_preview(s.handle_key(&key(KeyCode::Down), 10), &presets::DRACULA);
    assert_eq!(s.selected, 1);
}

#[test]
fn navigation_skips_rejected_rows_both_ways() {
    let mut s = fixture();
    s.selected = 1; // dracula; row 2 is rejected
    expect_preview(
        s.handle_key(&key(KeyCode::Down), 10),
        &presets::SOLARIZED_DARK,
    );
    assert_eq!(s.selected, 3);
    expect_preview(s.handle_key(&key(KeyCode::Up), 10), &presets::DRACULA);
    assert_eq!(s.selected, 1);
}

#[test]
fn enter_applies_selected_row() {
    let mut s = fixture();
    s.selected = 1;
    match s.handle_key(&key(KeyCode::Enter), 10) {
        PickerAction::Apply(t) => assert!(std::ptr::eq(t, &presets::DRACULA)),
        other => panic!("expected Apply, got {other:?}"),
    }
}

#[test]
fn enter_on_rejected_row_is_noop() {
    let mut s = fixture();
    s.selected = 2;
    expect_noop(s.handle_key(&key(KeyCode::Enter), 10));
}

#[test]
fn esc_and_q_cancel() {
    let mut s = fixture();
    expect_cancel(s.handle_key(&key(KeyCode::Esc), 10));
    expect_cancel(s.handle_key(&key(KeyCode::Char('q')), 10));
}

#[test]
fn movement_clamps_at_list_ends() {
    let mut s = fixture();
    expect_noop(s.handle_key(&key(KeyCode::Up), 10));
    assert_eq!(s.selected, 0);
    s.selected = 3;
    expect_noop(s.handle_key(&key(KeyCode::Down), 10));
    assert_eq!(s.selected, 3);
}

#[test]
fn empty_list_movement_is_safe() {
    let mut s = ThemePickerState {
        items: vec![],
        selected: 0,
        origin: &theme::CRAB_DARK,
        scroll_offset: 0,
    };
    expect_noop(s.handle_key(&key(KeyCode::Down), 10));
    assert_eq!(s.selected, 0);
}

#[test]
fn build_items_lands_cursor_on_active_theme() {
    let report = user_themes::LoadReport {
        themes: vec![],
        rejected: vec![],
    };
    let (items, selected) =
        build_items(&[&theme::CRAB_DARK, &presets::DRACULA], &report, "dracula");
    assert_eq!(selected, 1);
    assert_eq!(items.len(), 2);
}

#[test]
fn build_items_surfaces_rejected_files_with_reason() {
    let report = user_themes::LoadReport {
        themes: vec![],
        rejected: vec![user_themes::RejectedPreset {
            file: "broken.toml".to_string(),
            reason: "invalid hex".to_string(),
        }],
    };
    let (items, selected) = build_items(&[&theme::CRAB_DARK], &report, "crab-dark");
    assert_eq!(selected, 0);
    assert_eq!(items.len(), 2);
    assert!(items[1].theme.is_none());
    let reason = items[1].reason.as_deref().unwrap_or_default();
    assert!(reason.contains("broken.toml") && reason.contains("invalid hex"));
}

#[test]
fn scroll_window_follows_selection() {
    let mut s = fixture();
    for _ in 0..3 {
        s.handle_key(&key(KeyCode::Down), 2); // 2-row visible window
    }
    assert_eq!(s.selected, 3);
    assert!(s.selected >= s.scroll_offset);
    assert!(s.selected < s.scroll_offset + 2);
}
