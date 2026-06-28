//! Tests for the RSI built-in-tool ban guard (#236).
//!
//! The RSI escalated `hashline_edit` to a "blanket DO NOT USE" directive over
//! recoverable failures. The guard rejects brain-file content that bans a
//! built-in tool, while still allowing nuanced routing guidance and command
//! rules.

use crate::brain::tools::catalog::is_protected_builtin;
use crate::brain::tools::self_improve_guards::bans_builtin_tool;

#[test]
fn protected_builtins_include_the_reported_tools() {
    assert!(is_protected_builtin("hashline_edit"));
    assert!(is_protected_builtin("follow_up_question"));
    assert!(is_protected_builtin("telegram_send"));
    assert!(is_protected_builtin("bash"));
    assert!(is_protected_builtin("browser_navigate"));
}

#[test]
fn dynamic_and_unknown_tools_are_not_protected() {
    // User-defined / unknown tools may legitimately be disabled.
    assert!(!is_protected_builtin("gh_issue_list"));
    assert!(!is_protected_builtin("some_custom_tool"));
}

#[test]
fn rejects_blanket_ban_of_a_builtin() {
    let content = "### hashline_edit\nThis tool is fundamentally unreliable — DO NOT USE it; \
                   use edit_file exclusively.";
    let reason = bans_builtin_tool(content).expect("must reject a built-in ban");
    assert!(reason.contains("hashline_edit"), "reason: {reason}");
}

#[test]
fn rejects_never_use_of_a_channel_tool() {
    let content = "telegram_send keeps failing, so never use telegram_send for anything.";
    assert!(bans_builtin_tool(content).is_some());
}

#[test]
fn allows_routing_guidance_without_a_ban_phrase() {
    // "Prefer X over Y for case Z" is exactly the kind of guidance RSI SHOULD
    // write — no blanket prohibition, so it passes.
    let content = "Prefer edit_file over hashline_edit when editing files larger than 500 lines.";
    assert_eq!(bans_builtin_tool(content), None);
}

#[test]
fn allows_command_rules_that_are_not_tool_bans() {
    // A ban phrase aimed at a shell command (not a tool) must not trip the
    // guard — `git add -A` is not a protected tool name.
    let content = "Never use `git add -A` — stage files explicitly for atomic commits.";
    assert_eq!(bans_builtin_tool(content), None);
}
