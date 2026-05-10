//! Regression coverage for the pure helpers in
//! `src/brain/agent/service/tool_loop.rs`.
//!
//! Linor's 2026-05-04 repo audit flagged tool_loop.rs as the highest-
//! priority bug-risk hotspot: 3,731 lines, 167 commits, zero dedicated
//! test files. This module pins the small handful of pure helpers
//! exposed for testing — `strip_ansi_output`, `extract_path_for_
//! recent_buffer`, and `is_user_correction` — so that future
//! refactors of the surrounding loop don't silently break the
//! corner-cases they handle.
//!
//! Pure-helper coverage is the entry point for the bigger refactor
//! described in the Linor report (extracting cohesive concerns out of
//! the giant `run_tool_loop_inner`); the loop logic itself is too
//! intertwined with provider streams + DB writes to unit-test in
//! isolation, but at minimum we want every PURE function the loop
//! calls to have characterization tests that prevent regressions.
//!
//! When the bigger refactor lands and more pure functions surface,
//! add their tests to this file rather than splintering into many
//! tiny ones — the project rule (memory: tests live under
//! `src/tests/`) keeps the test surface flat.

use crate::brain::agent::service::tool_loop::{
    extract_path_for_recent_buffer, is_user_correction, strip_ansi_output,
};
use serde_json::json;
use std::path::PathBuf;

// ── strip_ansi_output ──────────────────────────────────────────────

#[test]
fn strip_ansi_output_removes_basic_color_codes() {
    // SGR 31 = red; SGR 0 = reset. Any reasonable ANSI stripper should
    // collapse this to the bare text.
    let raw = "\x1b[31merror\x1b[0m: tool failed";
    let cleaned = strip_ansi_output(raw);
    assert_eq!(cleaned, "error: tool failed");
}

#[test]
fn strip_ansi_output_handles_no_ansi() {
    let raw = "plain output, no escapes here";
    let cleaned = strip_ansi_output(raw);
    assert_eq!(cleaned, raw);
}

#[test]
fn strip_ansi_output_handles_empty_string() {
    assert_eq!(strip_ansi_output(""), "");
}

#[test]
fn strip_ansi_output_strips_cursor_movement() {
    // CSI 2J (clear screen), CSI H (cursor home) etc. should not leak
    // into tool-result text rendered to the model.
    let raw = "\x1b[2J\x1b[Hready";
    assert_eq!(strip_ansi_output(raw), "ready");
}

#[test]
fn strip_ansi_output_preserves_unicode() {
    // Non-ANSI multi-byte content must round-trip unchanged. Caught a
    // class of stripper bugs where ESC-detection code accidentally
    // consumed a UTF-8 continuation byte.
    let raw = "🦀 OpenCrabs";
    assert_eq!(strip_ansi_output(raw), "🦀 OpenCrabs");
}

#[test]
fn strip_ansi_output_strips_24bit_truecolor() {
    // SGR 38;2;R;G;B sets 24-bit foreground. Real-world tool output
    // (cargo, ripgrep, --color=always git diff) emits these.
    let raw = "\x1b[38;2;255;100;0morange\x1b[0m";
    assert_eq!(strip_ansi_output(raw), "orange");
}

// ── extract_path_for_recent_buffer ────────────────────────────────

fn cwd() -> PathBuf {
    std::env::temp_dir().join("opencrabs-tool-loop-test")
}

#[test]
fn extract_path_returns_none_for_unrelated_tools() {
    // Only tools that operate on a single file path are tracked.
    // bash / glob / web_search / generate_image have no path arg
    // worth surfacing.
    for tool in &["bash", "glob", "web_search", "generate_image", "task"] {
        let result = extract_path_for_recent_buffer(tool, &json!({}), &cwd());
        assert!(
            result.is_none(),
            "tool '{tool}' should never contribute to recent_paths"
        );
    }
}

#[test]
fn extract_path_returns_none_when_path_missing() {
    // read_file IS path-bearing, but if the agent emitted a malformed
    // call without the field, we must NOT inject a phantom row.
    let result = extract_path_for_recent_buffer("read_file", &json!({}), &cwd());
    assert!(result.is_none());
}

#[test]
fn extract_path_returns_none_for_empty_path() {
    // Whitespace-only paths are agent slop — same outcome as missing.
    for empty in &["", " ", "  \t\n  "] {
        let result = extract_path_for_recent_buffer("read_file", &json!({ "path": empty }), &cwd());
        assert!(
            result.is_none(),
            "empty path '{empty:?}' must not be recorded"
        );
    }
}

#[test]
fn extract_path_resolves_relative_against_cwd() {
    // The agent often returns relative paths. The recent-paths buffer
    // stores absolute paths so re-display next turn doesn't depend on
    // the working directory at lookup time.
    let result =
        extract_path_for_recent_buffer("read_file", &json!({ "path": "src/main.rs" }), &cwd());
    let path = result.expect("relative path must resolve");
    assert!(
        path.is_absolute(),
        "stored path should be absolute, got {path:?}"
    );
    assert!(
        path.ends_with("src/main.rs"),
        "tail must preserve the original path, got {path:?}"
    );
}

#[test]
#[cfg(unix)]
fn extract_path_passes_through_absolute_path() {
    // Already-absolute paths must round-trip unchanged.
    let result =
        extract_path_for_recent_buffer("read_file", &json!({ "path": "/etc/hosts" }), &cwd());
    assert_eq!(result, Some(PathBuf::from("/etc/hosts")));
}

#[test]
#[cfg(windows)]
fn extract_path_passes_through_absolute_path() {
    // On Windows, absolute paths require a drive letter.
    let result =
        extract_path_for_recent_buffer("read_file", &json!({ "path": "C:\\Windows\\System32\\drivers\\etc\\hosts" }), &cwd());
    assert_eq!(result, Some(PathBuf::from("C:\\Windows\\System32\\drivers\\etc\\hosts")));
}

#[test]
#[cfg(unix)]
fn extract_path_covers_all_documented_tools() {
    // Pin the documented set of path-bearing tools (read_file, edit_file,
    // write_file, ls, grep). If any are removed silently the recent-
    // paths buffer would stop tracking those operations and the next-
    // turn anchor list would go stale.
    for tool in &["read_file", "edit_file", "write_file", "ls", "grep"] {
        let result = extract_path_for_recent_buffer(tool, &json!({ "path": "/abs/file" }), &cwd());
        assert_eq!(
            result,
            Some(PathBuf::from("/abs/file")),
            "tool '{tool}' must contribute to recent_paths"
        );
    }
}

#[test]
#[cfg(windows)]
fn extract_path_covers_all_documented_tools() {
    // On Windows, absolute paths require a drive letter.
    for tool in &["read_file", "edit_file", "write_file", "ls", "grep"] {
        let result = extract_path_for_recent_buffer(tool, &json!({ "path": "C:\\abs\\file" }), &cwd());
        assert_eq!(
            result,
            Some(PathBuf::from("C:\\abs\\file")),
            "tool '{tool}' must contribute to recent_paths"
        );
    }
}

// ── is_user_correction ─────────────────────────────────────────────

#[test]
fn user_correction_detects_short_no_phrases() {
    // The most common correction signals — terse user pushback. These
    // should ALL trigger the retry-context injection.
    for msg in &[
        "no, that's wrong",
        "no.",
        "no!",
        "no that's not what I asked",
        "nope",
        "wrong",
        "that's not right",
        "that's wrong",
        "thats wrong",
        "not what i wanted",
        "try again",
        "redo",
        "revert",
        "undo",
        "you broke it",
        "broke it",
        "doesn't work",
        "doesnt work",
        "didn't work",
        "didnt work",
        "not working",
        "stop",
        "don't do that",
        "dont do that",
        "i said no",
        "i asked for X",
        "not correct",
        "fix it",
        "fix this",
    ] {
        assert!(
            is_user_correction(msg),
            "message {msg:?} should be detected as a correction"
        );
    }
}

#[test]
fn user_correction_ignores_long_messages() {
    // Long messages are usually new instructions, not corrections —
    // even when they contain a "no" or "stop" somewhere.
    let long = "I have a new task involving the API surface for our \
                ingestion pipeline. Please don't do the same approach as \
                last time — that hit a deadlock under concurrent load. \
                Instead, write a server in Rust that responds to GET \
                requests with JSON. Use Tokio + axum. Implement /health, \
                /metrics, /readyz, /livez, and /version endpoints. Don't \
                include any frontend assets, TLS termination, or auth \
                middleware. Stop after the server is running locally and \
                report back with the full route table, the listening \
                address, and the binary footprint. Make sure tests pass.";
    assert!(
        long.len() > 500,
        "test fixture must actually be long ({}) for the gate to be exercised",
        long.len()
    );
    assert!(
        !is_user_correction(long),
        "long instruction must NOT be classified as correction"
    );
}

#[test]
fn user_correction_ignores_extremely_short_messages() {
    // Anything under 2 chars can't carry an unambiguous correction
    // signal — must not trigger a retry on a single-keystroke message.
    for msg in &["", "?"] {
        assert!(
            !is_user_correction(msg),
            "extremely short message {msg:?} must not trigger correction path"
        );
    }
}

#[test]
fn user_correction_is_case_insensitive() {
    // Patterns are matched against lowercased input — uppercase user
    // shouting must still be detected.
    for msg in &["NO!", "WRONG", "Try Again", "FIX IT"] {
        assert!(
            is_user_correction(msg),
            "uppercase {msg:?} must be detected"
        );
    }
}

#[test]
fn user_correction_does_not_fire_on_neutral_prose() {
    // Common phrasing in normal user prompts should NOT trigger the
    // retry-context injection; otherwise routine messages get treated
    // as failures.
    for msg in &[
        "what is the capital of France?",
        "show me the diff",
        "let's add a new feature",
        "explain how this works",
        "thanks",
        "ok",
        "got it",
    ] {
        assert!(
            !is_user_correction(msg),
            "neutral message {msg:?} must NOT be classified as correction"
        );
    }
}

#[test]
fn user_correction_only_scans_first_300_chars() {
    // The is_user_correction function explicitly takes only the first
    // 300 chars before lowercasing — pin that so a "stop" word buried
    // deep in a long message doesn't false-trigger.
    let prefix = "x".repeat(300);
    let buried = format!("{prefix} stop please");
    // Must not exceed the 500-char total cap, otherwise the length gate
    // discards before we hit the prefix scan.
    assert!(buried.len() <= 500);
    assert!(
        !is_user_correction(&buried),
        "trigger word past 300-char window must not match"
    );
}
