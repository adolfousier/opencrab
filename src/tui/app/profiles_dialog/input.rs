//! Keyboard handling for `AppMode::Profiles`.
//!
//! Dialog model: a filterable profile list with action modes for
//! create, delete confirm, and migrate. Type-to-narrow in browse mode;
//! dedicated input fields for create/migrate flows.
//!
//! `decide` is pure (mutates `ProfilesDialogState`, returns `KeyOutcome`)
//! so the keystroke contract is unit-testable without spinning up a
//! full `App`.

use super::state::{ProfileAction, ProfilesDialogState, matching};
use crate::tui::app::App;
use crate::tui::events::AppMode;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Effect of a keystroke that the wrapper has to apply at the App level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyOutcome {
    /// Key consumed; no further App-level action required.
    Consumed,
    /// User wants to leave the dialog, caller should switch back to Chat.
    Close,
    /// Switch to the profile at this index.
    Switch(usize),
    /// Delete the profile at this index (after confirmation).
    Delete(usize),
    /// Create a new profile with the given name and optional description.
    Create(String, Option<String>),
    /// Migrate from source to destination.
    Migrate(String, String),
    /// Key not recognised; caller may fall through to default handlers.
    NotConsumed,
}

/// Top-level handler called from the App keystroke dispatcher.
pub async fn handle_key(app: &mut App, key: KeyEvent) {
    let profiles = app.profiles_dialog.profiles.clone();
    let active = app.profiles_dialog.active_profile.clone();
    match decide(&mut app.profiles_dialog, &profiles, &active, key) {
        KeyOutcome::Consumed | KeyOutcome::NotConsumed => {}
        KeyOutcome::Close => {
            app.mode = AppMode::Chat;
        }
        KeyOutcome::Switch(idx) => {
            let visible = matching(&profiles, &app.profiles_dialog.filter);
            if let Some(entry) = visible.get(idx) {
                let name = entry.name.clone();
                super::actions::switch_to(app, &name);
            }
        }
        KeyOutcome::Delete(idx) => {
            let visible = matching(&profiles, &app.profiles_dialog.filter);
            if let Some(entry) = visible.get(idx) {
                let name = entry.name.clone();
                super::actions::delete(app, &name);
            }
        }
        KeyOutcome::Create(name, desc) => {
            super::actions::create(app, &name, desc.as_deref());
        }
        KeyOutcome::Migrate(from, to) => {
            super::actions::migrate(app, &from, &to);
        }
    }
}

/// Pure decision function. Mutates `state`; returns the App-level effect.
pub fn decide(
    state: &mut ProfilesDialogState,
    profiles: &[crate::config::profile::ProfileEntry],
    active: &str,
    key: KeyEvent,
) -> KeyOutcome {
    // Global: Esc always closes or cancels current action
    if key.code == KeyCode::Esc {
        if state.action != ProfileAction::None {
            state.action = ProfileAction::None;
            state.input_buffer.clear();
            state.input_buffer_2.clear();
            return KeyOutcome::Consumed;
        }
        return KeyOutcome::Close;
    }

    // Action-specific input modes
    match &state.action {
        ProfileAction::CreateName => {
            return handle_text_input(state, key, ProfileAction::CreateDesc);
        }
        ProfileAction::CreateDesc => {
            return handle_create_desc(state, key);
        }
        ProfileAction::ConfirmDelete(name) => {
            return handle_confirm_delete(state, name.clone(), key);
        }
        ProfileAction::MigrateFrom => {
            return handle_text_input(state, key, ProfileAction::MigrateTo);
        }
        ProfileAction::MigrateTo => {
            return handle_migrate_to(state, key);
        }
        ProfileAction::None => {}
    }

    // Browse mode
    match key.code {
        KeyCode::Tab | KeyCode::Down => {
            move_selection(state, profiles, 1);
            KeyOutcome::Consumed
        }
        KeyCode::BackTab | KeyCode::Up => {
            move_selection(state, profiles, -1);
            KeyOutcome::Consumed
        }

        KeyCode::Enter => {
            let visible = matching(profiles, &state.filter);
            if let Some(entry) = visible.get(state.selected_index) {
                if entry.name == active {
                    KeyOutcome::Consumed // already active
                } else {
                    KeyOutcome::Switch(state.selected_index)
                }
            } else {
                KeyOutcome::Consumed
            }
        }

        KeyCode::Char('n') if key.modifiers.is_empty() => {
            state.action = ProfileAction::CreateName;
            state.input_buffer.clear();
            KeyOutcome::Consumed
        }
        KeyCode::Char('d') if key.modifiers.is_empty() => {
            let visible = matching(profiles, &state.filter);
            if let Some(entry) = visible.get(state.selected_index) {
                if entry.name == "default" {
                    return KeyOutcome::Consumed; // can't delete default
                }
                state.action = ProfileAction::ConfirmDelete(entry.name.clone());
            }
            KeyOutcome::Consumed
        }
        KeyCode::Char('m') if key.modifiers.is_empty() => {
            state.action = ProfileAction::MigrateFrom;
            state.input_buffer.clear();
            state.input_buffer_2.clear();
            KeyOutcome::Consumed
        }

        KeyCode::Backspace => {
            state.filter.pop();
            state.selected_index = 0;
            KeyOutcome::Consumed
        }

        KeyCode::Char(c) => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                return KeyOutcome::NotConsumed;
            }
            state.filter.push(c);
            state.selected_index = 0;
            KeyOutcome::Consumed
        }

        _ => KeyOutcome::NotConsumed,
    }
}

/// Handle text input for single-field actions (create name, migrate from).
/// On Enter, advances to `next_action`.
fn handle_text_input(
    state: &mut ProfilesDialogState,
    key: KeyEvent,
    next_action: ProfileAction,
) -> KeyOutcome {
    match key.code {
        KeyCode::Enter => {
            if state.input_buffer.trim().is_empty() {
                return KeyOutcome::Consumed;
            }
            state.action = next_action;
            KeyOutcome::Consumed
        }
        KeyCode::Backspace => {
            state.input_buffer.pop();
            KeyOutcome::Consumed
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.input_buffer.push(c);
            KeyOutcome::Consumed
        }
        _ => KeyOutcome::Consumed,
    }
}

/// Handle the create description step. Enter finalizes.
fn handle_create_desc(state: &mut ProfilesDialogState, key: KeyEvent) -> KeyOutcome {
    match key.code {
        KeyCode::Enter => {
            let name = state.input_buffer.trim().to_string();
            if name.is_empty() {
                return KeyOutcome::Consumed;
            }
            let desc = if state.input_buffer_2.trim().is_empty() {
                None
            } else {
                Some(state.input_buffer_2.trim().to_string())
            };
            KeyOutcome::Create(name, desc)
        }
        KeyCode::Backspace => {
            state.input_buffer_2.pop();
            KeyOutcome::Consumed
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.input_buffer_2.push(c);
            KeyOutcome::Consumed
        }
        _ => KeyOutcome::Consumed,
    }
}

/// Handle delete confirmation. 'y' or Enter confirms, anything else cancels.
fn handle_confirm_delete(
    state: &mut ProfilesDialogState,
    _name: String,
    key: KeyEvent,
) -> KeyOutcome {
    match key.code {
        KeyCode::Char('y') | KeyCode::Enter => {
            let idx = state.selected_index;
            state.action = ProfileAction::None;
            KeyOutcome::Delete(idx)
        }
        _ => {
            state.action = ProfileAction::None;
            KeyOutcome::Consumed
        }
    }
}

/// Handle the migrate destination step. Enter finalizes.
fn handle_migrate_to(state: &mut ProfilesDialogState, key: KeyEvent) -> KeyOutcome {
    match key.code {
        KeyCode::Enter => {
            let from = state.input_buffer.trim().to_string();
            let to = state.input_buffer_2.trim().to_string();
            if from.is_empty() || to.is_empty() {
                return KeyOutcome::Consumed;
            }
            KeyOutcome::Migrate(from, to)
        }
        KeyCode::Backspace => {
            state.input_buffer_2.pop();
            KeyOutcome::Consumed
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.input_buffer_2.push(c);
            KeyOutcome::Consumed
        }
        _ => KeyOutcome::Consumed,
    }
}

fn move_selection(
    state: &mut ProfilesDialogState,
    profiles: &[crate::config::profile::ProfileEntry],
    delta: i32,
) {
    let visible = matching(profiles, &state.filter);
    let count = visible.len();
    if count == 0 {
        state.selected_index = 0;
        return;
    }
    let count_i = count as i32;
    let cur = state.selected_index.min(count - 1) as i32;
    let next = ((cur + delta) % count_i + count_i) % count_i;
    state.selected_index = next as usize;
}
