//! Keyboard handling for `AppMode::MissionControl`.
//!
//! Three layers:
//!
//!  1. Detail popup open → Esc closes the popup. j/k still scroll the
//!     selection underneath so the popup updates as the user moves.
//!  2. Panel focused, no popup → Tab/Shift-Tab cycle panels;
//!     j/k or ↑/↓ move selection within the focused panel; Enter
//!     opens the detail popup; Esc closes MC entirely.
//!  3. Apply / reject (`a` / `r`) land in C12 alongside the
//!     `rsi_proposals` action plumbing.
//!
//! The decision logic is split into a pure `decide` function that takes
//! a `&mut McState` plus the current panel item count, and returns a
//! `KeyOutcome`. The `handle_key` wrapper at the top routes that
//! outcome back into `App`-level effects (mode switch on Close). This
//! keeps the keystroke logic unit-testable without spinning up a full
//! `App`.

use super::state::{McPanel, McState};
use crate::tui::app::App;
use crate::tui::events::AppMode;
use crossterm::event::{KeyCode, KeyEvent};

/// Effect of a keystroke that the wrapper has to apply at the App level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyOutcome {
    /// Key consumed; no further App-level action required.
    Consumed,
    /// User wants to leave MC — caller should switch back to Chat mode.
    Close,
    /// Key wasn't recognised; caller may fall through to the chat-mode
    /// default handlers.
    NotConsumed,
    /// Inbox panel is focused and the user pressed `a` — caller should
    /// apply the currently selected proposal via `actions::apply_selected`.
    ApplySelected,
    /// Inbox panel is focused and the user pressed `r` — caller should
    /// reject the currently selected proposal.
    RejectSelected,
}

/// Top-level handler called from the App's keystroke dispatcher.
/// Mission Control is a full-screen mode (like `Sessions` and `Help`) —
/// the App's match-arm doesn't fall through to chat handlers, so there
/// is no return value to thread back. Unrecognised keys are simply
/// ignored.
pub async fn handle_key(app: &mut App, key: KeyEvent) {
    let count = panel_count(app);
    match decide(&mut app.mc, count, key) {
        KeyOutcome::Consumed | KeyOutcome::NotConsumed => {}
        KeyOutcome::Close => {
            app.mode = AppMode::Chat;
            app.mc.detail_open = false;
        }
        KeyOutcome::ApplySelected => super::actions::apply_selected(app).await,
        KeyOutcome::RejectSelected => super::actions::reject_selected(app).await,
    }
}

/// Pure decision function — mutates `state`, returns the App-level
/// effect. `panel_item_count` is the number of items in the currently
/// focused panel, used to clamp selection movement.
pub fn decide(state: &mut McState, panel_item_count: usize, key: KeyEvent) -> KeyOutcome {
    if state.detail_open {
        decide_with_popup(state, panel_item_count, key)
    } else {
        decide_without_popup(state, panel_item_count, key)
    }
}

fn decide_with_popup(state: &mut McState, panel_item_count: usize, key: KeyEvent) -> KeyOutcome {
    match key.code {
        KeyCode::Esc => {
            state.detail_open = false;
            KeyOutcome::Consumed
        }
        // Allow scrolling the underlying selection so the popup updates
        // as the user moves through the list.
        KeyCode::Up | KeyCode::Char('k') => {
            move_selection(state, panel_item_count, -1);
            KeyOutcome::Consumed
        }
        KeyCode::Down | KeyCode::Char('j') => {
            move_selection(state, panel_item_count, 1);
            KeyOutcome::Consumed
        }
        _ => KeyOutcome::NotConsumed,
    }
}

fn decide_without_popup(state: &mut McState, panel_item_count: usize, key: KeyEvent) -> KeyOutcome {
    match key.code {
        KeyCode::Esc => KeyOutcome::Close,
        KeyCode::Tab | KeyCode::Char('l') => {
            state.focus_next();
            KeyOutcome::Consumed
        }
        KeyCode::BackTab | KeyCode::Char('h') => {
            state.focus_prev();
            KeyOutcome::Consumed
        }
        KeyCode::Up | KeyCode::Char('k') => {
            move_selection(state, panel_item_count, -1);
            KeyOutcome::Consumed
        }
        KeyCode::Down | KeyCode::Char('j') => {
            move_selection(state, panel_item_count, 1);
            KeyOutcome::Consumed
        }
        KeyCode::Home | KeyCode::Char('g') => {
            state.selected_index = 0;
            KeyOutcome::Consumed
        }
        KeyCode::End | KeyCode::Char('G') => {
            state.selected_index = panel_item_count.saturating_sub(1);
            KeyOutcome::Consumed
        }
        KeyCode::Enter => {
            if panel_item_count > 0 {
                state.detail_open = true;
            }
            KeyOutcome::Consumed
        }
        // Apply / reject are scoped to the Inbox panel — that's where
        // the actionable items live. On other panels these keys are
        // swallowed (Consumed) rather than falling through, so they
        // can't accidentally trigger anything in the chat handlers.
        KeyCode::Char('a') => {
            if state.focused_panel == McPanel::Inbox && panel_item_count > 0 {
                KeyOutcome::ApplySelected
            } else {
                KeyOutcome::Consumed
            }
        }
        KeyCode::Char('r') => {
            if state.focused_panel == McPanel::Inbox && panel_item_count > 0 {
                KeyOutcome::RejectSelected
            } else {
                KeyOutcome::Consumed
            }
        }
        _ => KeyOutcome::NotConsumed,
    }
}

fn move_selection(state: &mut McState, count: usize, delta: i32) {
    if count == 0 {
        state.selected_index = 0;
        return;
    }
    let max_idx = count - 1;
    let cur = state.selected_index.min(max_idx) as i32;
    let next = (cur + delta).clamp(0, max_idx as i32) as usize;
    state.selected_index = next;
}

fn panel_count(app: &App) -> usize {
    match app.mc.focused_panel {
        // Inbox count is recomputed each draw from the proposals store
        // rather than cached in McState. Reading it here means a
        // fresh disk read on every Enter / Tab keystroke, which stays
        // in sync if the inbox file changes mid-session.
        McPanel::Inbox => crate::brain::mission_control::inbox_service::list().len(),
        McPanel::Activity => app.mc.activity.len(),
        McPanel::Schedule => app.mc.schedule.len(),
        // One "item" (the whole snapshot) so Enter opens the full-detail popup
        // like the other panels; there are no per-row selections.
        McPanel::Analytics => 1,
    }
}
