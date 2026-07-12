//! Side-effect actions invoked from the skills dialog.

use crate::tui::app::App;
use crate::tui::events::{AppMode, TuiEvent};

/// Open the skills dialog. Resets filter + selection so re-opening
/// always lands on the unfiltered top of the list.
pub fn open(app: &mut App) {
    if app.mode == AppMode::SkillsList {
        return;
    }
    app.mode = AppMode::SkillsList;
    app.skills_dialog.reset();
}

/// Send `body` as a prompt to the agent — same path the slash-command
/// dispatcher uses for skill auto-registration. Caller has already
/// flipped `app.mode` back to Chat before invoking this so the prompt
/// lands in the chat surface.
pub fn execute(app: &App, body: String) {
    let _ = app.event_sender().send(TuiEvent::CommandSubmitted(body));
}
