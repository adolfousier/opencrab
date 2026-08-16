//! Tests for tool-failure classification (#236).
//!
//! Recoverable/environmental/user-driven failures must be classified so the
//! ledger keeps them out of the success-rate denominator; genuine defects must
//! still count.

use crate::brain::feedback_policy::is_recoverable_tool_failure;

#[test]
fn hashline_stale_hash_is_recoverable() {
    assert!(is_recoverable_tool_failure(
        "hashline_edit",
        Some("Hash #EJ not found in file. The file may have changed since your last read.")
    ));
    assert!(is_recoverable_tool_failure(
        "hashline_edit",
        Some("Hash collision on #NH across 9 lines")
    ));
}

#[test]
fn channel_send_not_connected_is_recoverable() {
    assert!(is_recoverable_tool_failure(
        "telegram_send",
        Some("Telegram is not connected. Ask the user to connect Telegram first.")
    ));
    assert!(is_recoverable_tool_failure(
        "whatsapp_send",
        Some("not connected")
    ));
    assert!(is_recoverable_tool_failure(
        "discord_connect",
        Some("failed to connect to gateway")
    ));
}

#[test]
fn follow_up_question_cancel_is_recoverable() {
    assert!(is_recoverable_tool_failure(
        "follow_up_question",
        Some("user cancelled the prompt")
    ));
    assert!(is_recoverable_tool_failure(
        "follow_up_question",
        Some("timed out waiting for an answer")
    ));
    assert!(is_recoverable_tool_failure(
        "follow_up_question",
        Some("user declined")
    ));
}

#[test]
fn genuine_defects_are_not_recoverable() {
    // A real bug in a tool's own logic must still count as a failure.
    assert!(!is_recoverable_tool_failure(
        "hashline_edit",
        Some("internal panic: index out of bounds")
    ));
    // bash failing a command is a real outcome, not an exempt class.
    assert!(!is_recoverable_tool_failure(
        "bash",
        Some("command not found: frobnicate")
    ));
    // telegram_send with an unknown-action misuse is a real (learnable) error,
    // not the environmental "not connected" class.
    assert!(!is_recoverable_tool_failure(
        "telegram_send",
        Some("unknown action 'list_channels'")
    ));
}

#[test]
fn environmental_bash_failures_are_recoverable() {
    // #1068: bash had no arm at all, so a well-formed command that hit a
    // missing file or a dead service counted against the tool exactly like a
    // command the model got wrong. That made `tool_failure|bash` the ledger's
    // loudest signal and pointed RSI at "bash is broken".
    assert!(is_recoverable_tool_failure(
        "bash",
        Some(
            "Command exited with code 1\n\n-- output captured before error --\n\
             STDERR:\ncat: /etc/nope.conf: No such file or directory\n"
        )
    ));
    assert!(is_recoverable_tool_failure(
        "bash",
        Some(
            "Command exited with code 7\n\n-- output captured before error --\n\
             STDERR:\ncurl: (7) Failed to connect to localhost port 8931: Connection refused\n"
        )
    ));
}

#[test]
fn bash_commands_the_model_got_wrong_stay_failures() {
    // The other half of the split, and the reason the classifier checks model
    // markers first: these are agent behaviour RSI is supposed to act on.
    for stderr in [
        "bash: line 3: syntax error near unexpected token `fi'",
        "bash: line 1: MY_VAR: unbound variable",
        "ls: illegal option -- Z",
    ] {
        assert!(
            !is_recoverable_tool_failure(
                "bash",
                Some(&format!(
                    "Command exited with code 2\n\n-- output captured before error --\n\
                     STDERR:\n{stderr}\n"
                ))
            ),
            "must stay a failure: {stderr}"
        );
    }
}

#[test]
fn missing_error_text_is_not_recoverable() {
    // No error string → cannot be classified benign; must count normally.
    assert!(!is_recoverable_tool_failure("hashline_edit", None));
    assert!(!is_recoverable_tool_failure("follow_up_question", None));
}

#[test]
fn unrelated_tools_are_never_auto_recoverable() {
    // A tool not in the policy list must never be silently exempted, even if
    // its error happens to contain a keyword.
    assert!(!is_recoverable_tool_failure(
        "read_file",
        Some("file not found")
    ));
}
