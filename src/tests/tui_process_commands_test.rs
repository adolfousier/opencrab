//! `/restart` and `/exit` on the TUI surface (#923).
//!
//! Both existed on channels but not here, so typing either did nothing and the
//! `/help` dialog did not list them. The dialog renders straight from
//! `SLASH_COMMANDS` (`src/tui/render/help.rs:84` iterates `cmd.name`), so
//! registry membership is what makes a command discoverable. A command that
//! dispatches but is unregistered is invisible; one registered but not
//! dispatched is documented and dead. These assert the registry half.

use crate::tui::app::state::SLASH_COMMANDS;

fn registered(name: &str) -> bool {
    SLASH_COMMANDS.iter().any(|c| c.name == name)
}

#[test]
fn restart_and_exit_are_registered() {
    assert!(
        registered("/restart"),
        "/restart missing from SLASH_COMMANDS"
    );
    assert!(registered("/exit"), "/exit missing from SLASH_COMMANDS");
}

#[test]
fn registered_commands_describe_themselves() {
    // An empty description renders a blank row in the help dialog, which reads
    // as a broken entry rather than a command with no summary.
    for cmd in SLASH_COMMANDS {
        assert!(
            !cmd.description.trim().is_empty(),
            "{} has no description",
            cmd.name
        );
        assert!(
            cmd.name.starts_with('/'),
            "{} is not a slash command",
            cmd.name
        );
    }
}

#[test]
fn no_duplicate_slash_commands() {
    // A duplicate shows twice in the dialog and makes autocomplete ambiguous.
    let mut seen: Vec<&str> = SLASH_COMMANDS.iter().map(|c| c.name).collect();
    let before = seen.len();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(before, seen.len(), "SLASH_COMMANDS contains a duplicate");
}
