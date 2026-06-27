//! Tests for the `/profiles` dialog — filter behaviour, navigation,
//! keystroke contract, and action flows (create, delete, migrate).
//!
//! Drives the pure `decide` fn against a `ProfilesDialogState` directly
//! — no full `App` needed.

use crate::config::profile::ProfileEntry;
use crate::tui::app::profiles_dialog::input::{KeyOutcome, decide};
use crate::tui::app::profiles_dialog::state::{ProfileAction, ProfilesDialogState, matching};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn entry(name: &str, description: Option<&str>) -> ProfileEntry {
    ProfileEntry {
        name: name.to_string(),
        description: description.map(String::from),
        created_at: "2026-06-27T00:00:00Z".to_string(),
        last_used: None,
    }
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::empty())
}

fn ctrl_key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

// ── Filter behaviour ────────────────────────────────────────────────────────

#[test]
fn empty_filter_returns_every_profile() {
    let profiles = vec![
        entry("default", Some("Default profile")),
        entry("hermes", Some("Messenger")),
        entry("scout", None),
    ];
    let visible = matching(&profiles, "");
    assert_eq!(visible.len(), 3);
}

#[test]
fn filter_matches_substring_of_name() {
    let profiles = vec![
        entry("hermes", Some("Messenger")),
        entry("scout", None),
        entry("hercules", Some("Strong")),
    ];
    let visible = matching(&profiles, "herm");
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].name, "hermes");
}

#[test]
fn filter_is_case_insensitive() {
    let profiles = vec![entry("Hermes", Some("Messenger"))];
    let visible = matching(&profiles, "hermes");
    assert_eq!(visible.len(), 1);
}

#[test]
fn filter_no_match_returns_empty() {
    let profiles = vec![entry("alpha", None), entry("beta", None)];
    let visible = matching(&profiles, "zzz");
    assert!(visible.is_empty());
}

#[test]
fn filter_preserves_order() {
    let profiles = vec![
        entry("alpha", None),
        entry("beta", None),
        entry("gamma", None),
    ];
    let visible = matching(&profiles, "");
    assert_eq!(visible[0].name, "alpha");
    assert_eq!(visible[1].name, "beta");
    assert_eq!(visible[2].name, "gamma");
}

#[test]
fn filter_matches_multiple_substring() {
    let profiles = vec![
        entry("test-alpha", None),
        entry("test-beta", None),
        entry("prod", None),
    ];
    let visible = matching(&profiles, "test");
    assert_eq!(visible.len(), 2);
}

// ── Type-to-filter keystroke flow ──────────────────────────────────────────

#[test]
fn typing_a_char_appends_to_filter_and_resets_selection() {
    let mut s = ProfilesDialogState {
        selected_index: 4,
        ..Default::default()
    };
    let out = decide(&mut s, &[], "default", key(KeyCode::Char('a')));
    assert_eq!(out, KeyOutcome::Consumed);
    assert_eq!(s.filter, "a");
    assert_eq!(
        s.selected_index, 0,
        "selection must reset to top when filter changes"
    );
}

#[test]
fn backspace_pops_last_char_and_resets_selection() {
    let mut s = ProfilesDialogState {
        filter: "abc".to_string(),
        selected_index: 3,
        ..Default::default()
    };
    decide(&mut s, &[], "default", key(KeyCode::Backspace));
    assert_eq!(s.filter, "ab");
    assert_eq!(s.selected_index, 0);
}

#[test]
fn ctrl_chars_are_not_consumed_as_filter_input() {
    let mut s = ProfilesDialogState::default();
    let out = decide(&mut s, &[], "default", ctrl_key('c'));
    assert_eq!(out, KeyOutcome::NotConsumed);
    assert!(
        s.filter.is_empty(),
        "Ctrl-C should never end up in the filter buffer"
    );
}

// ── Navigation ──────────────────────────────────────────────────────────────

#[test]
fn tab_advances_selection_within_filtered_count() {
    let profiles = vec![
        entry("alpha", None),
        entry("beta", None),
        entry("gamma", None),
    ];
    let mut s = ProfilesDialogState::default();
    decide(&mut s, &profiles, "default", key(KeyCode::Tab));
    assert_eq!(s.selected_index, 1);
    decide(&mut s, &profiles, "default", key(KeyCode::Down));
    assert_eq!(s.selected_index, 2);
}

#[test]
fn down_at_last_wraps_to_first() {
    let profiles = vec![entry("alpha", None), entry("beta", None)];
    let mut s = ProfilesDialogState {
        selected_index: 1,
        ..Default::default()
    };
    decide(&mut s, &profiles, "default", key(KeyCode::Down));
    assert_eq!(s.selected_index, 0, "Down at last should wrap to first");
}

#[test]
fn tab_at_last_wraps_to_first() {
    let profiles = vec![
        entry("alpha", None),
        entry("beta", None),
        entry("gamma", None),
    ];
    let mut s = ProfilesDialogState {
        selected_index: 2,
        ..Default::default()
    };
    decide(&mut s, &profiles, "default", key(KeyCode::Tab));
    assert_eq!(s.selected_index, 0);
}

#[test]
fn up_at_first_wraps_to_last() {
    let profiles = vec![
        entry("alpha", None),
        entry("beta", None),
        entry("gamma", None),
    ];
    let mut s = ProfilesDialogState::default();
    decide(&mut s, &profiles, "default", key(KeyCode::Up));
    assert_eq!(s.selected_index, 2, "Up at 0 should wrap to last");
}

#[test]
fn back_tab_at_first_wraps_to_last() {
    let profiles = vec![entry("alpha", None), entry("beta", None)];
    let mut s = ProfilesDialogState::default();
    decide(&mut s, &profiles, "default", key(KeyCode::BackTab));
    assert_eq!(s.selected_index, 1);
}

#[test]
fn back_tab_goes_backward() {
    let profiles = vec![entry("alpha", None), entry("beta", None)];
    let mut s = ProfilesDialogState {
        selected_index: 1,
        ..Default::default()
    };
    decide(&mut s, &profiles, "default", key(KeyCode::BackTab));
    assert_eq!(s.selected_index, 0);
}

#[test]
fn navigation_uses_filtered_count_not_total() {
    // Total of 3 profiles, filter narrows to 1 match. Down should clamp
    // at index 0, not advance into the filtered-out entries.
    let profiles = vec![
        entry("alpha", None),
        entry("beta", None),
        entry("gamma", None),
    ];
    let mut s = ProfilesDialogState {
        filter: "beta".to_string(),
        ..Default::default()
    };
    decide(&mut s, &profiles, "default", key(KeyCode::Down));
    assert_eq!(s.selected_index, 0, "single match — selection stays pinned");
}

// ── Enter & Esc ─────────────────────────────────────────────────────────────

#[test]
fn enter_on_inactive_profile_returns_switch() {
    let profiles = vec![
        entry("default", Some("Default profile")),
        entry("hermes", Some("Messenger")),
    ];
    let mut s = ProfilesDialogState::default();
    // Select hermes (index 1)
    decide(&mut s, &profiles, "default", key(KeyCode::Tab));
    let out = decide(&mut s, &profiles, "default", key(KeyCode::Enter));
    assert_eq!(out, KeyOutcome::Switch(1));
}

#[test]
fn enter_on_active_profile_is_consumed_silently() {
    let profiles = vec![
        entry("default", Some("Default profile")),
        entry("hermes", Some("Messenger")),
    ];
    let mut s = ProfilesDialogState::default();
    // Select default (index 0) which is also the active profile
    let out = decide(&mut s, &profiles, "default", key(KeyCode::Enter));
    assert_eq!(out, KeyOutcome::Consumed);
}

#[test]
fn enter_with_empty_filtered_list_is_consumed_silently() {
    let profiles: Vec<ProfileEntry> = Vec::new();
    let mut s = ProfilesDialogState::default();
    let out = decide(&mut s, &profiles, "default", key(KeyCode::Enter));
    assert_eq!(out, KeyOutcome::Consumed);
}

#[test]
fn esc_in_browse_returns_close() {
    let mut s = ProfilesDialogState::default();
    let out = decide(&mut s, &[], "default", key(KeyCode::Esc));
    assert_eq!(out, KeyOutcome::Close);
}

#[test]
fn esc_in_action_returns_to_browse() {
    let mut s = ProfilesDialogState {
        action: ProfileAction::CreateName,
        input_buffer: "test".to_string(),
        ..Default::default()
    };
    let out = decide(&mut s, &[], "default", key(KeyCode::Esc));
    assert_eq!(out, KeyOutcome::Consumed);
    assert_eq!(s.action, ProfileAction::None);
    assert!(s.input_buffer.is_empty());
}

// ── Delete flow ─────────────────────────────────────────────────────────────

#[test]
fn d_key_starts_confirm_delete_for_non_default() {
    let profiles = vec![
        entry("default", Some("Default profile")),
        entry("hermes", Some("Messenger")),
    ];
    let mut s = ProfilesDialogState::default();
    // Select hermes (index 1)
    decide(&mut s, &profiles, "default", key(KeyCode::Tab));
    decide(&mut s, &profiles, "default", key(KeyCode::Char('d')));
    assert_eq!(s.action, ProfileAction::ConfirmDelete("hermes".to_string()));
}

#[test]
fn d_key_on_default_is_noop() {
    let profiles = vec![
        entry("default", Some("Default profile")),
        entry("hermes", Some("Messenger")),
    ];
    let mut s = ProfilesDialogState::default();
    // Index 0 is default
    decide(&mut s, &profiles, "default", key(KeyCode::Char('d')));
    assert_eq!(s.action, ProfileAction::None);
}

#[test]
fn confirm_delete_y_returns_delete() {
    let profiles = vec![
        entry("default", Some("Default profile")),
        entry("hermes", Some("Messenger")),
    ];
    let mut s = ProfilesDialogState {
        action: ProfileAction::ConfirmDelete("hermes".to_string()),
        selected_index: 1,
        ..Default::default()
    };
    let out = decide(&mut s, &profiles, "default", key(KeyCode::Char('y')));
    assert_eq!(out, KeyOutcome::Delete(1));
    assert_eq!(s.action, ProfileAction::None);
}

#[test]
fn confirm_delete_enter_returns_delete() {
    let profiles = vec![
        entry("default", Some("Default profile")),
        entry("hermes", Some("Messenger")),
    ];
    let mut s = ProfilesDialogState {
        action: ProfileAction::ConfirmDelete("hermes".to_string()),
        selected_index: 1,
        ..Default::default()
    };
    let out = decide(&mut s, &profiles, "default", key(KeyCode::Enter));
    assert_eq!(out, KeyOutcome::Delete(1));
}

#[test]
fn confirm_delete_other_key_cancels() {
    let mut s = ProfilesDialogState {
        action: ProfileAction::ConfirmDelete("hermes".to_string()),
        selected_index: 1,
        ..Default::default()
    };
    let out = decide(&mut s, &[], "default", key(KeyCode::Char('n')));
    assert_eq!(out, KeyOutcome::Consumed);
    assert_eq!(s.action, ProfileAction::None);
}

// ── Create flow ─────────────────────────────────────────────────────────────

#[test]
fn n_key_enters_create_name_mode() {
    let mut s = ProfilesDialogState::default();
    let out = decide(&mut s, &[], "default", key(KeyCode::Char('n')));
    assert_eq!(out, KeyOutcome::Consumed);
    assert_eq!(s.action, ProfileAction::CreateName);
}

#[test]
fn create_name_types_into_input_buffer() {
    let mut s = ProfilesDialogState {
        action: ProfileAction::CreateName,
        ..Default::default()
    };
    for c in "test".chars() {
        decide(&mut s, &[], "default", key(KeyCode::Char(c)));
    }
    assert_eq!(s.input_buffer, "test");
}

#[test]
fn create_name_enter_advances_to_create_desc() {
    let mut s = ProfilesDialogState {
        action: ProfileAction::CreateName,
        input_buffer: "my-profile".to_string(),
        ..Default::default()
    };
    let out = decide(&mut s, &[], "default", key(KeyCode::Enter));
    assert_eq!(out, KeyOutcome::Consumed);
    assert_eq!(s.action, ProfileAction::CreateDesc);
}

#[test]
fn create_name_enter_empty_is_noop() {
    let mut s = ProfilesDialogState {
        action: ProfileAction::CreateName,
        input_buffer: "".to_string(),
        ..Default::default()
    };
    let out = decide(&mut s, &[], "default", key(KeyCode::Enter));
    assert_eq!(out, KeyOutcome::Consumed);
    assert_eq!(
        s.action,
        ProfileAction::CreateName,
        "should stay in CreateName when buffer is empty"
    );
}

#[test]
fn create_name_enter_whitespace_only_is_noop() {
    let mut s = ProfilesDialogState {
        action: ProfileAction::CreateName,
        input_buffer: "   ".to_string(),
        ..Default::default()
    };
    let out = decide(&mut s, &[], "default", key(KeyCode::Enter));
    assert_eq!(out, KeyOutcome::Consumed);
    assert_eq!(
        s.action,
        ProfileAction::CreateName,
        "should stay in CreateName when buffer is whitespace"
    );
}

#[test]
fn create_desc_enter_returns_create() {
    let mut s = ProfilesDialogState {
        action: ProfileAction::CreateDesc,
        input_buffer: "my-profile".to_string(),
        input_buffer_2: "A test profile".to_string(),
        ..Default::default()
    };
    let out = decide(&mut s, &[], "default", key(KeyCode::Enter));
    assert_eq!(
        out,
        KeyOutcome::Create("my-profile".to_string(), Some("A test profile".to_string()))
    );
}

#[test]
fn create_desc_enter_no_description_returns_create_with_none() {
    let mut s = ProfilesDialogState {
        action: ProfileAction::CreateDesc,
        input_buffer: "my-profile".to_string(),
        input_buffer_2: "".to_string(),
        ..Default::default()
    };
    let out = decide(&mut s, &[], "default", key(KeyCode::Enter));
    assert_eq!(out, KeyOutcome::Create("my-profile".to_string(), None));
}

#[test]
fn create_desc_types_into_input_buffer_2() {
    let mut s = ProfilesDialogState {
        action: ProfileAction::CreateDesc,
        input_buffer: "my-profile".to_string(),
        ..Default::default()
    };
    for c in "A test".chars() {
        decide(&mut s, &[], "default", key(KeyCode::Char(c)));
    }
    assert_eq!(s.input_buffer_2, "A test");
}

#[test]
fn create_desc_backspace_pops_buffer_2() {
    let mut s = ProfilesDialogState {
        action: ProfileAction::CreateDesc,
        input_buffer: "my-profile".to_string(),
        input_buffer_2: "A test".to_string(),
        ..Default::default()
    };
    decide(&mut s, &[], "default", key(KeyCode::Backspace));
    assert_eq!(s.input_buffer_2, "A tes");
}

// ── Migrate flow ────────────────────────────────────────────────────────────

#[test]
fn m_key_enters_migrate_from_mode() {
    let mut s = ProfilesDialogState::default();
    let out = decide(&mut s, &[], "default", key(KeyCode::Char('m')));
    assert_eq!(out, KeyOutcome::Consumed);
    assert_eq!(s.action, ProfileAction::MigrateFrom);
    assert!(s.input_buffer.is_empty());
    assert!(s.input_buffer_2.is_empty());
}

#[test]
fn migrate_from_enter_advances_to_migrate_to() {
    let mut s = ProfilesDialogState {
        action: ProfileAction::MigrateFrom,
        input_buffer: "source".to_string(),
        ..Default::default()
    };
    let out = decide(&mut s, &[], "default", key(KeyCode::Enter));
    assert_eq!(out, KeyOutcome::Consumed);
    assert_eq!(s.action, ProfileAction::MigrateTo);
}

#[test]
fn migrate_from_enter_empty_is_noop() {
    let mut s = ProfilesDialogState {
        action: ProfileAction::MigrateFrom,
        input_buffer: "".to_string(),
        ..Default::default()
    };
    let out = decide(&mut s, &[], "default", key(KeyCode::Enter));
    assert_eq!(out, KeyOutcome::Consumed);
    assert_eq!(s.action, ProfileAction::MigrateFrom);
}

#[test]
fn migrate_to_enter_returns_migrate() {
    let mut s = ProfilesDialogState {
        action: ProfileAction::MigrateTo,
        input_buffer: "source".to_string(),
        input_buffer_2: "dest".to_string(),
        ..Default::default()
    };
    let out = decide(&mut s, &[], "default", key(KeyCode::Enter));
    assert_eq!(
        out,
        KeyOutcome::Migrate("source".to_string(), "dest".to_string())
    );
}

#[test]
fn migrate_to_enter_empty_from_is_noop() {
    let mut s = ProfilesDialogState {
        action: ProfileAction::MigrateTo,
        input_buffer: "".to_string(),
        input_buffer_2: "dest".to_string(),
        ..Default::default()
    };
    let out = decide(&mut s, &[], "default", key(KeyCode::Enter));
    assert_eq!(out, KeyOutcome::Consumed);
}

#[test]
fn migrate_to_enter_empty_to_is_noop() {
    let mut s = ProfilesDialogState {
        action: ProfileAction::MigrateTo,
        input_buffer: "source".to_string(),
        input_buffer_2: "".to_string(),
        ..Default::default()
    };
    let out = decide(&mut s, &[], "default", key(KeyCode::Enter));
    assert_eq!(out, KeyOutcome::Consumed);
}

// ── Auto-focus on unique match ──────────────────────────────────────────────

#[test]
fn typing_until_one_match_leaves_first_index_focused_and_switchable() {
    let profiles = vec![
        entry("alpha-profile", Some("First")),
        entry("beta-profile", None),
    ];
    // Set filter directly — 'm', 'n', 'd' are browse-mode hotkeys so
    // simulating keystrokes would trigger action modes instead of filtering.
    let mut s = ProfilesDialogState {
        filter: "alph".to_string(),
        ..Default::default()
    };
    let visible = matching(&profiles, &s.filter);
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].name, "alpha-profile");
    let out = decide(&mut s, &profiles, "default", key(KeyCode::Enter));
    assert_eq!(out, KeyOutcome::Switch(0));
}

// ── State reset ─────────────────────────────────────────────────────────────

#[test]
fn reset_clears_all_state() {
    let mut s = ProfilesDialogState {
        filter: "test".to_string(),
        selected_index: 3,
        scroll_offset: 10,
        action: ProfileAction::CreateName,
        input_buffer: "buf".to_string(),
        input_buffer_2: "buf2".to_string(),
        ..Default::default()
    };
    s.reset();
    assert!(s.filter.is_empty());
    assert_eq!(s.selected_index, 0);
    assert_eq!(s.scroll_offset, 0);
    assert_eq!(s.action, ProfileAction::None);
    assert!(s.input_buffer.is_empty());
    assert!(s.input_buffer_2.is_empty());
}

// ── ProfileAction default ───────────────────────────────────────────────────

#[test]
fn profile_action_default_is_none() {
    assert_eq!(ProfileAction::default(), ProfileAction::None);
}

// ── Edge cases ──────────────────────────────────────────────────────────────

#[test]
fn all_ctrl_keys_not_consumed_in_browse() {
    let mut s = ProfilesDialogState::default();
    for c in ['a', 'x', 'v', 'z', 'q'] {
        s.filter.clear();
        let out = decide(&mut s, &[], "default", ctrl_key(c));
        assert_eq!(
            out,
            KeyOutcome::NotConsumed,
            "Ctrl-{} should be NotConsumed",
            c
        );
        assert!(s.filter.is_empty(), "Ctrl-{} should not go into filter", c);
    }
}

#[test]
fn backspace_on_empty_filter_is_noop() {
    let mut s = ProfilesDialogState::default();
    let out = decide(&mut s, &[], "default", key(KeyCode::Backspace));
    assert_eq!(out, KeyOutcome::Consumed);
    assert!(s.filter.is_empty());
}

#[test]
fn enter_on_active_profile_with_filter_returns_consumed() {
    let profiles = vec![
        entry("default", Some("Default profile")),
        entry("hermes", Some("Messenger")),
    ];
    let mut s = ProfilesDialogState {
        filter: "default".to_string(),
        ..Default::default()
    };
    let out = decide(&mut s, &profiles, "default", key(KeyCode::Enter));
    assert_eq!(out, KeyOutcome::Consumed);
}

#[test]
fn create_name_backspace_pops_buffer() {
    let mut s = ProfilesDialogState {
        action: ProfileAction::CreateName,
        input_buffer: "abc".to_string(),
        ..Default::default()
    };
    decide(&mut s, &[], "default", key(KeyCode::Backspace));
    assert_eq!(s.input_buffer, "ab");
}

#[test]
fn create_name_ctrl_chars_not_consumed() {
    let mut s = ProfilesDialogState {
        action: ProfileAction::CreateName,
        ..Default::default()
    };
    let out = decide(&mut s, &[], "default", ctrl_key('c'));
    assert_eq!(out, KeyOutcome::Consumed);
    assert!(s.input_buffer.is_empty());
}
