//! Loop-guard normalization (#957).
//!
//! The Luna incident (18+ near-identical announcements, bash echo loop)
//! slipped past every existing guard because they all match EXACT text or
//! EXACT tool signatures. Normalization collapses counters, punctuation and
//! whitespace so near-identical repeats become exact matches; the bash
//! near-match in the tool loop builds directly on it, and the cross-turn
//! announcement ring buffer (which lands with its own module) layers
//! Jaccard near-duplicate matching on top.

use crate::brain::agent::service::helpers::normalize_loop_text;

#[test]
fn normalize_strips_digits_punctuation_and_lowercases() {
    assert_eq!(
        normalize_loop_text("Echo \"Hello, World!\" 42 times -- really?!"),
        "echo hello world times really"
    );
}

#[test]
fn normalize_collapses_whitespace_and_caps_length() {
    assert_eq!(normalize_loop_text("a   b\n\nc"), "a b c");
    let long = "word ".repeat(500);
    assert!(normalize_loop_text(&long).chars().count() <= 400);
}

#[test]
fn normalize_keeps_cyrillic_letters() {
    // Alexey's Luna log was Russian; the guard must normalize it cleanly.
    assert_eq!(
        normalize_loop_text("Отправляю 6 подтверждений в ДДС!"),
        "отправляю подтверждений в ддс"
    );
}

#[test]
fn counter_variations_collide_after_normalization() {
    // The Luna pattern: same bash echo, only the counter moves.
    // Normalization must make them EXACTLY equal — that is what the
    // tool-loop bash near-match relies on.
    let a = "bash: echo \"Отправляю 1 подтверждение в ДДС\"";
    let b = "bash: echo \"Отправляю 2 подтверждение в ДДС\"";
    assert_eq!(normalize_loop_text(a), normalize_loop_text(b));

    let c = "bash: echo \"Sending confirmation 3 of 6 to DDS with the PDF attachment\"";
    let d = "bash: echo \"Sending confirmation 4 of 6 to DDS with the PDF attachment\"";
    assert_eq!(normalize_loop_text(c), normalize_loop_text(d));
}

#[test]
fn different_commands_stay_apart_after_normalization() {
    assert_ne!(
        normalize_loop_text("cargo build --release"),
        normalize_loop_text("cargo test --all-features")
    );
    assert_ne!(
        normalize_loop_text("git status"),
        normalize_loop_text("ls -la")
    );
}
