//! Keyboard handling for `AppMode::SkillsList`.
//!
//! Dialog model: a single filterable list. The text input layer is
//! implicit — every printable char that isn't a recognised navigation
//! key appends to the filter, which lets type-to-narrow work without
//! an explicit focus toggle. Tab / Shift-Tab / ↑ / ↓ / j / k navigate
//! the filtered list; Enter executes the selected skill; Esc closes.
//!
//! `decide` is pure (mutates `SkillsDialogState`, returns `KeyOutcome`)
//! so the keystroke contract is unit-testable without spinning up a
//! full `App`.

use super::state::{SkillsDialogState, matching};
use crate::brain::skills::Skill;
use crate::tui::app::App;
use crate::tui::events::AppMode;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Effect of a keystroke that the wrapper has to apply at the App level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyOutcome {
    /// Key consumed; no further App-level action required.
    Consumed,
    /// User wants to leave the dialog — caller should switch back to Chat.
    Close,
    /// User pressed Enter on the selected skill — caller should send
    /// the skill body as a prompt and close the dialog.
    Execute(String),
    /// Key wasn't recognised; caller may fall through to default
    /// handlers (which won't fire in a full-screen mode anyway).
    NotConsumed,
}

/// Top-level handler called from the App's keystroke dispatcher.
pub async fn handle_key(app: &mut App, key: KeyEvent) {
    let skills = app.skills.clone();
    match decide(&mut app.skills_dialog, &skills, key) {
        KeyOutcome::Consumed | KeyOutcome::NotConsumed => {}
        KeyOutcome::Close => {
            app.mode = AppMode::Chat;
        }
        KeyOutcome::Execute(body) => {
            app.mode = AppMode::Chat;
            super::actions::execute(app, body);
        }
    }
}

/// Pure decision function. Mutates `state`; returns the App-level
/// effect. `skills` is the unfiltered loaded set — the function does
/// the filtering internally so the caller doesn't have to thread the
/// filter result through.
pub fn decide(state: &mut SkillsDialogState, skills: &[Skill], key: KeyEvent) -> KeyOutcome {
    match key.code {
        KeyCode::Esc => KeyOutcome::Close,

        KeyCode::Enter => {
            let visible = matching(skills, &state.filter);
            match visible.get(state.selected_index) {
                // `prompt_body()` carries the review-gate reminder for
                // flagged skills, same as the `/name` slash path.
                Some(s) => KeyOutcome::Execute(s.prompt_body()),
                None => KeyOutcome::Consumed,
            }
        }

        KeyCode::Tab | KeyCode::Down => {
            move_selection(state, skills, 1);
            KeyOutcome::Consumed
        }
        KeyCode::BackTab | KeyCode::Up => {
            move_selection(state, skills, -1);
            KeyOutcome::Consumed
        }

        KeyCode::Backspace => {
            // Backspace pops the last char off the filter and resets
            // selection to 0 — keeps "first match" highlighted as the
            // user trims back the query.
            state.filter.pop();
            state.selected_index = 0;
            KeyOutcome::Consumed
        }

        KeyCode::Char(c) => {
            // Filter input. Skip the Ctrl-modified variants — those
            // are reserved for shortcuts the parent App may handle.
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

fn move_selection(state: &mut SkillsDialogState, skills: &[Skill], delta: i32) {
    let visible = matching(skills, &state.filter);
    let count = visible.len();
    if count == 0 {
        state.selected_index = 0;
        return;
    }
    // Wrap around on Tab / Shift-Tab / ↑ / ↓ so the user can keep
    // pressing the same key to cycle. The double-modulo trick is the
    // standard Rust positive-modulo idiom (Rust's `%` is remainder,
    // not Euclidean modulo, so negative deltas need the extra step).
    let count_i = count as i32;
    let cur = state.selected_index.min(count - 1) as i32;
    let next = ((cur + delta) % count_i + count_i) % count_i;
    state.selected_index = next as usize;
}
