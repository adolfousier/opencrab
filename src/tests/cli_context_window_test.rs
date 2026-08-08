//! CLI providers must honour `providers.<name>.context_window` (#973).
//!
//! The README states the override "works on every provider" and names the CLI
//! providers explicitly. It did not: all four hardcoded a constant in
//! `context_window()` while already storing the user's value in
//! `configured_context_window` and exposing it through the trait. The value was
//! parsed, passed to the provider, stored, and then ignored by the one method
//! that answers the question.

use crate::brain::provider::Provider;
use crate::brain::provider::claude_cli::ClaudeCliProvider;
use crate::brain::provider::codex_cli::CodexCliProvider;
use crate::brain::provider::command_code_cli::CommandCodeCliProvider;
use crate::brain::provider::opencode_cli::OpenCodeCliProvider;

/// The reported case: a 1M window configured for Claude CLI.
const ONE_MILLION: u32 = 1_000_000;

#[test]
fn claude_cli_honours_the_configured_window() {
    let Ok(p) = ClaudeCliProvider::new() else {
        return; // binary absent on this machine; nothing to assert
    };
    assert_eq!(p.context_window("opus-5"), Some(200_000), "default");

    let p = p.with_context_window(ONE_MILLION);
    assert_eq!(p.context_window("opus-5"), Some(ONE_MILLION));
    assert_eq!(p.configured_context_window(), Some(ONE_MILLION));
}

#[test]
fn opencode_cli_honours_the_configured_window() {
    let Ok(p) = OpenCodeCliProvider::new() else {
        return;
    };
    assert_eq!(p.context_window("any"), Some(128_000), "default");

    let p = p.with_context_window(ONE_MILLION);
    assert_eq!(p.context_window("any"), Some(ONE_MILLION));
}

#[test]
fn codex_cli_honours_the_configured_window() {
    let Ok(p) = CodexCliProvider::new() else {
        return;
    };
    assert_eq!(p.context_window("any"), Some(200_000), "default");

    let p = p.with_context_window(ONE_MILLION);
    assert_eq!(p.context_window("any"), Some(ONE_MILLION));
}

#[test]
fn command_code_cli_honours_the_configured_window() {
    let Ok(p) = CommandCodeCliProvider::new() else {
        return;
    };
    assert_eq!(p.context_window("any"), Some(200_000), "default");

    let p = p.with_context_window(ONE_MILLION);
    assert_eq!(p.context_window("any"), Some(ONE_MILLION));
}

#[test]
fn a_smaller_override_is_honoured_too() {
    // The override is not a "raise only" knob: someone capping a provider
    // below its default must get the cap they asked for.
    let Ok(p) = ClaudeCliProvider::new() else {
        return;
    };
    let p = p.with_context_window(32_000);
    assert_eq!(p.context_window("opus-5"), Some(32_000));
}
