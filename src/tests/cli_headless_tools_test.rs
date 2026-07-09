//! Regression tests for headless CLI tool execution (#492).
//!
//! `opencrabs run --auto-approve` and `opencrabs agent -m` must run the full
//! tool loop like every channel surface, not a single completion that prints
//! tool-call syntax as text. These guard both the approval-answer parsing and
//! the wiring in the CLI command handlers.

use crate::cli::headless_callbacks::approval_from_answer;

#[test]
fn approval_answer_yes_approves_once() {
    assert_eq!(approval_from_answer("y"), (true, false));
    assert_eq!(approval_from_answer("yes"), (true, false));
    assert_eq!(approval_from_answer("  YES \n"), (true, false));
}

#[test]
fn approval_answer_always_approves_and_sticks() {
    assert_eq!(approval_from_answer("a"), (true, true));
    assert_eq!(approval_from_answer("always"), (true, true));
    assert_eq!(approval_from_answer("Always"), (true, true));
}

#[test]
fn approval_answer_anything_else_denies() {
    assert_eq!(approval_from_answer("n"), (false, false));
    assert_eq!(approval_from_answer("no"), (false, false));
    assert_eq!(approval_from_answer(""), (false, false));
    assert_eq!(approval_from_answer("maybe"), (false, false));
}

/// The headless CLI handlers must route through the tool loop, never the bare
/// single-completion `send_message`. This is the exact regression from #492:
/// `run`/`agent` printed tool syntax as text because they called
/// `send_message` and skipped `run_tool_loop`.
#[test]
fn headless_cli_routes_through_tool_loop() {
    let src = include_str!("../cli/commands.rs");

    // Both run and agent paths invoke the tool-loop entry point.
    let loop_calls = src.matches("send_message_with_tools_and_callback").count();
    assert!(
        loop_calls >= 2,
        "expected cmd_run and cmd_agent_interactive to both call \
         send_message_with_tools_and_callback, found {loop_calls}"
    );

    // The --auto-approve flag must actually be wired, not discarded.
    assert!(
        src.contains(".with_auto_approve_tools(auto_approve)"),
        "auto_approve must be wired into the AgentService builder"
    );
    assert!(
        !src.contains("let _ = auto_approve;"),
        "auto_approve must not be discarded (the #492 TODO)"
    );
}
