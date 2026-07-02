//! Tests for `channels::commands::strip_command_handle` — removing the
//! `@botname` suffix Telegram appends to commands in groups (#265).
//!
//! The strip must touch only the command token: the old implementation cut
//! the whole line at the first `@`, discarding the arguments, so
//! `/respond_to@yourbot mention` became a bare `/respond_to` and showed the
//! current mode instead of switching it.

use crate::channels::commands::strip_command_handle;

#[test]
fn handle_with_argument_preserves_argument() {
    assert_eq!(
        strip_command_handle("/respond_to@yourbot mention"),
        "/respond_to mention"
    );
}

#[test]
fn handle_with_path_argument() {
    assert_eq!(strip_command_handle("/cd@yourbot /tmp"), "/cd /tmp");
}

#[test]
fn bare_command_with_handle() {
    assert_eq!(strip_command_handle("/help@yourbot"), "/help");
}

#[test]
fn command_without_handle_unchanged() {
    assert_eq!(
        strip_command_handle("/respond_to mention"),
        "/respond_to mention"
    );
}

#[test]
fn at_sign_in_argument_untouched() {
    // '@' beyond the command token must never truncate anything.
    assert_eq!(
        strip_command_handle("/sessions user@example.com"),
        "/sessions user@example.com"
    );
}

#[test]
fn handle_on_token_and_at_in_argument() {
    assert_eq!(
        strip_command_handle("/sessions@yourbot user@example.com"),
        "/sessions user@example.com"
    );
}

#[test]
fn non_command_text_untouched() {
    assert_eq!(strip_command_handle("hello @you"), "hello @you");
    assert_eq!(strip_command_handle("mail me at a@b.c"), "mail me at a@b.c");
}

#[test]
fn plain_command_unchanged() {
    assert_eq!(strip_command_handle("/help"), "/help");
}

#[test]
fn surrounding_whitespace_trimmed() {
    assert_eq!(strip_command_handle("  /help@yourbot  "), "/help");
    assert_eq!(strip_command_handle("  plain text  "), "plain text");
}

#[test]
fn multiline_argument_preserved() {
    // split_once(char::is_whitespace) also splits on newline — args keep
    // their remaining structure.
    assert_eq!(
        strip_command_handle("/cmd@yourbot line1\nline2"),
        "/cmd line1\nline2"
    );
}
