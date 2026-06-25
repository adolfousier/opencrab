//! Tests for Telegram command name sanitization and description truncation.
//!
//! These helpers ensure user-defined commands and skills are properly formatted
//! for Telegram's bot command API (max 100 commands, lowercase + underscores only,
//! descriptions capped at 256 chars).

use crate::channels::telegram::{sanitize_command_name, truncate_description};

#[test]
fn sanitize_converts_hyphens_to_underscores() {
    assert_eq!(sanitize_command_name("security-audit"), "security_audit");
    assert_eq!(sanitize_command_name("repo-audit"), "repo_audit");
    assert_eq!(sanitize_command_name("a2a-gateway"), "a2a_gateway");
}

/// The Telegram menu standard is the underscore form: every multi-word command
/// and skill is sanitized hyphen→underscore so the menu is consistent
/// (`mission_control`, `github_workflow`, …). The canonical dash is kept in
/// /help and accepted (alongside the underscore) by the dispatcher.
#[test]
fn menu_uses_consistent_underscore_form() {
    assert_eq!(sanitize_command_name("mission-control"), "mission_control");
    assert_eq!(sanitize_command_name("github-workflow"), "github_workflow");
    // Single-word and already-underscore names are unchanged.
    assert_eq!(sanitize_command_name("usage"), "usage");
    assert_eq!(sanitize_command_name("github_workflow"), "github_workflow");
}

#[test]
fn sanitize_converts_to_lowercase() {
    assert_eq!(sanitize_command_name("SecurityAudit"), "securityaudit");
    assert_eq!(sanitize_command_name("DEPLOY"), "deploy");
}

#[test]
fn sanitize_strips_invalid_characters() {
    assert_eq!(sanitize_command_name("cmd@name"), "cmdname");
    assert_eq!(sanitize_command_name("test!123"), "test123");
    assert_eq!(sanitize_command_name("a.b.c"), "abc");
}

#[test]
fn sanitize_preserves_underscores_and_alphanumeric() {
    assert_eq!(sanitize_command_name("my_command_123"), "my_command_123");
    assert_eq!(sanitize_command_name("abc123"), "abc123");
}

#[test]
fn sanitize_handles_empty_input() {
    assert_eq!(sanitize_command_name(""), "");
}

#[test]
fn sanitize_handles_single_character() {
    assert_eq!(sanitize_command_name("a"), "a");
    assert_eq!(sanitize_command_name("-"), "_");
}

#[test]
fn mission_control_becomes_valid_telegram_name() {
    // Regression: the hardcoded built-in `mission-control` wasn't sanitized,
    // so its hyphen made the WHOLE setMyCommands call fail (BOT_COMMAND_INVALID)
    // and the menu froze on the stale list (analytics, no mission-control).
    let name = sanitize_command_name("mission-control");
    assert_eq!(name, "mission_control");
    assert!(
        name.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
        "sanitized name must contain no hyphens or other invalid chars"
    );
}

#[test]
fn truncate_preserves_short_descriptions() {
    assert_eq!(truncate_description("Short desc", 256), "Short desc");
    assert_eq!(truncate_description("", 256), "");
}

#[test]
fn truncate_cuts_long_descriptions_with_ellipsis() {
    let long = "a".repeat(300);
    let result = truncate_description(&long, 256);
    assert_eq!(result.chars().count(), 256);
    assert!(result.ends_with('…'));
}

#[test]
fn truncate_handles_exact_max_length() {
    let exact = "x".repeat(256);
    assert_eq!(truncate_description(&exact, 256), exact);
}

#[test]
fn truncate_handles_one_over_max_length() {
    let over = "x".repeat(257);
    let result = truncate_description(&over, 256);
    assert_eq!(result.chars().count(), 256);
    assert!(result.ends_with('…'));
}
