//! Regression tests for the `/goal` slash command wiring (#232).
//!
//! Three things broke in practice and are pinned here:
//!
//! 1. The `slash_command` dispatch matches the command verbatim, so a model
//!    passing `command='/goal debug the build'` (everything in one field)
//!    fell through to the user-command path and failed. `normalize_command`
//!    splits the first token off so the call routes correctly regardless of
//!    how the model fills the two fields.
//!
//! 2. The shared `goal_command_prompt` directive must instruct
//!    `command='/goal'` with the rest in `args`, never `command='/goal …'`.
//!
//! 3. A bare `/goal` (no text) must be DENIED with a usage warning, never
//!    forwarded to the agent — `is_bare` gates that, and the warning is a
//!    display-only string.

use crate::brain::goal::{goal_command_prompt, goal_usage_warning, is_bare};
use crate::brain::tools::slash_command::normalize_command;

// ── normalize_command: the dispatch-robustness fix ──────────────────

#[test]
fn multiword_command_field_is_split_into_command_and_args() {
    // The exact failure from the screenshot: whole string in `command`.
    let (cmd, args) = normalize_command("/goal debug opencrabs last commits", "");
    assert_eq!(cmd, "/goal");
    assert_eq!(args, "debug opencrabs last commits");
}

#[test]
fn already_split_command_passes_through_unchanged() {
    let (cmd, args) = normalize_command("/goal", "debug the build");
    assert_eq!(cmd, "/goal");
    assert_eq!(args, "debug the build");
}

#[test]
fn bare_command_keeps_empty_args() {
    let (cmd, args) = normalize_command("/goal", "");
    assert_eq!(cmd, "/goal");
    assert_eq!(args, "");
}

#[test]
fn command_trailing_words_prepend_to_existing_args() {
    // Both fields populated: trailing words from `command` come first.
    let (cmd, args) = normalize_command("/goal set", "fix the flaky test");
    assert_eq!(cmd, "/goal");
    assert_eq!(args, "set fix the flaky test");
}

#[test]
fn whitespace_is_trimmed_on_both_fields() {
    let (cmd, args) = normalize_command("  /goal   status  ", "   ");
    assert_eq!(cmd, "/goal");
    assert_eq!(args, "status");
}

#[test]
fn normalize_is_generic_across_commands() {
    // Not goal-specific: /models with the model name stuffed into command.
    let (cmd, args) = normalize_command("/models gpt-5", "");
    assert_eq!(cmd, "/models");
    assert_eq!(args, "gpt-5");
}

// ── is_bare: the deny gate ──────────────────────────────────────────

#[test]
fn is_bare_true_for_empty_and_whitespace() {
    assert!(is_bare(""));
    assert!(is_bare("   "));
    assert!(is_bare("\t \n"));
}

#[test]
fn is_bare_false_when_text_present() {
    assert!(!is_bare("fix the build"));
    assert!(!is_bare("  status  "));
}

// ── goal_command_prompt: the shared directive ───────────────────────

#[test]
fn prompt_never_folds_args_into_command_field() {
    // The bug: directive said command='/goal <args>'. It must say
    // command='/goal' and args='<args>' separately.
    let p = goal_command_prompt("debug opencrabs last commits");
    assert!(
        p.contains("command='/goal'"),
        "directive must keep command as exactly /goal: {p}"
    );
    assert!(
        p.contains("args='debug opencrabs last commits'"),
        "directive must pass the goal text via args: {p}"
    );
    assert!(
        !p.contains("command='/goal debug"),
        "directive must NOT stuff the goal text into the command field: {p}"
    );
}

#[test]
fn prompt_trims_surrounding_whitespace_in_args() {
    let p = goal_command_prompt("   fix the build   ");
    assert!(p.contains("args='fix the build'"), "{p}");
}

// ── goal_usage_warning: the deny message ────────────────────────────

#[test]
fn usage_warning_shows_correct_shape() {
    let w = goal_usage_warning();
    // Tells the user the command-followed-by-goal shape.
    assert!(w.contains("/goal <your goal>"), "{w}");
    // Mentions the management subcommands.
    assert!(w.contains("/goal status"), "{w}");
    assert!(w.contains("/goal clear"), "{w}");
}

// ── #1167: autocorrect missing leading `/` ───────────────────────────

#[test]
fn missing_slash_is_prepended() {
    let (cmd, args) = normalize_command("status", "");
    assert_eq!(cmd, "/status");
    assert_eq!(args, "");

    let (cmd, args) = normalize_command("models list", "");
    assert_eq!(cmd, "/models");
    assert_eq!(args, "list");
}

#[test]
fn present_slash_is_untouched() {
    let (cmd, _) = normalize_command("/status", "");
    assert_eq!(cmd, "/status");
}

#[test]
fn empty_command_stays_empty_for_the_rejection_path() {
    // The execute() path still rejects empty commands; normalize must not
    // invent a bare "/" out of nothing.
    let (cmd, _) = normalize_command("", "");
    assert_eq!(cmd, "");
}
