//! Loop guard (#957): normalization, near-duplicate matching, and the
//! cross-turn outgoing-text ring.
//!
//! The Luna incident (18+ near-identical announcements, bash echo loop)
//! slipped past every existing guard because they all match EXACT text or
//! EXACT tool signatures within ONE turn. Normalization collapses
//! counters, punctuation and whitespace so near-identical repeats become
//! exact matches (tool layer); Jaccard near-duplicate matching over the
//! normalized word sets plus a per-session ring buffer catches the
//! reworded announcements that span turns (text layer).

use crate::brain::agent::service::announcement_loop::{
    OutgoingTextRing, TextLoopAction, near_duplicate,
};
use crate::brain::agent::service::helpers::normalize_loop_text;

// ---- normalize_loop_text ----

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

// ---- near_duplicate ----

#[test]
fn counter_variations_collide() {
    // The Luna pattern: same bash echo, only the counter moves.
    let a = "echo \"Отправляю 1 подтверждение в ДДС\"";
    let b = "echo \"Отправляю 2 подтверждение в ДДС\"";
    assert!(near_duplicate(a, b));

    let c = "Sending confirmation 3 of 6 to DDS with the PDF attachment";
    let d = "Sending confirmation 4 of 6 to DDS with the PDF attachment";
    assert!(near_duplicate(c, d));
}

#[test]
fn different_commands_do_not_collide() {
    assert!(!near_duplicate(
        "cargo build --release",
        "cargo test --all-features"
    ));
    assert!(!near_duplicate("git status", "ls -la"));
}

#[test]
fn short_texts_require_exact_normalized_match() {
    // Below 3 normalized words, only equality counts (Jaccard too coarse).
    assert!(near_duplicate("echo hi", "echo hi!"));
    assert!(!near_duplicate("echo hi", "echo yo"));
}

#[test]
fn similar_but_different_reports_do_not_collide() {
    // Legitimate templated status reports that differ where it matters.
    let a = "Deployed backend v1.2 to prod, all checks green";
    let b = "Deployed frontend v3.4 to staging, all checks green";
    assert!(!near_duplicate(a, b));
}

#[test]
fn empty_or_digit_only_text_is_not_a_duplicate() {
    assert!(!near_duplicate("", ""));
    assert!(!near_duplicate("12345", "12345"));
}

// ---- OutgoingTextRing (text layer) ----

#[test]
fn trip_sequence_nudges_then_aborts() {
    // The approved #957 acceptance sequence: clean -> clean -> trip-nudge
    // -> trip-abort. Counter-only variants count as near-duplicates.
    let mut ring = OutgoingTextRing::default();
    let texts = [
        "Отправляю 1 подтверждение по документу в ДДС и продолжаю обработку, ожидайте",
        "Отправляю 2 подтверждение по документу в ДДС и продолжаю обработку, ожидайте",
        "Отправляю 3 подтверждение по документу в ДДС и продолжаю обработку, ожидайте",
        "Отправляю 4 подтверждение по документу в ДДС и продолжаю обработку, ожидайте",
    ];
    assert_eq!(ring.record_and_check(texts[0]), TextLoopAction::Continue);
    assert_eq!(ring.record_and_check(texts[1]), TextLoopAction::Continue);
    assert_eq!(ring.record_and_check(texts[2]), TextLoopAction::Nudge);
    assert_eq!(ring.record_and_check(texts[3]), TextLoopAction::Abort);
}

#[test]
fn varied_genuine_texts_never_trip() {
    // No-false-positive: legitimately different turn outputs, including
    // templated status reports that differ where it matters.
    let mut ring = OutgoingTextRing::default();
    let texts = [
        "Deployed backend v1.2 to prod, all 42 checks green",
        "Deployed frontend v3.4 to staging, all 51 checks green",
        "Ran cargo clippy across the workspace, zero warnings",
        "Merged the session-store refactor after review",
        "Scheduled the nightly backup rotation job",
        "Summarized the standup notes and posted them",
    ];
    for t in texts {
        assert_eq!(
            ring.record_and_check(t),
            TextLoopAction::Continue,
            "text: {t}"
        );
    }
}

#[test]
fn ring_rotation_still_trips_after_old_entries_drop() {
    // The ring caps at 5: fill it with distinct texts first, then start
    // the loop. Old distinct entries fall out and the near-duplicates
    // still reach the trip threshold.
    let mut ring = OutgoingTextRing::default();
    for t in [
        "First unrelated answer about the config audit",
        "Second unrelated answer with the benchmark numbers",
        "Third unrelated answer summarizing the migration",
        "Fourth unrelated answer about the flaky test fix",
        "Fifth unrelated answer closing out the review",
    ] {
        assert_eq!(ring.record_and_check(t), TextLoopAction::Continue);
    }
    let loopy = "Отправляю подтверждение по документу в ДДС и продолжаю обработку, ожидайте ";
    let first = format!("{loopy}1");
    let second = format!("{loopy}2");
    let third = format!("{loopy}3");
    assert_eq!(ring.record_and_check(&first), TextLoopAction::Continue);
    assert_eq!(ring.record_and_check(&second), TextLoopAction::Continue);
    assert_eq!(ring.record_and_check(&third), TextLoopAction::Nudge);
}

// ---- Luna fixture regression (#957) ----

const LUNA_FIXTURE: &str = include_str!("fixtures/luna_echo_loop.txt");

fn fixture_lines(prefix: &str) -> Vec<String> {
    LUNA_FIXTURE
        .lines()
        .filter(|l| !l.trim_start().starts_with('#') && l.starts_with(prefix))
        .map(|l| l[prefix.len()..].to_string())
        .collect()
}

#[test]
fn luna_bash_echoes_all_normalize_identically() {
    // Tool layer: every echo differs only by its counter, so all
    // normalized commands must collide — that is exactly what the
    // bash near-match in the tool loop counts.
    let cmds = fixture_lines("bash|");
    assert!(cmds.len() >= 6, "fixture must carry the full echo loop");
    let mut normalized = cmds.iter().map(|c| normalize_loop_text(c));
    let first = normalized.next().unwrap();
    assert!(!first.is_empty());
    for n in normalized {
        assert_eq!(n, first, "counter variant escaped the tool-layer net");
    }
}

#[test]
fn luna_announcements_trip_the_ring() {
    // Text layer: feeding Luna's reworded announcements through the ring
    // must nudge once and then abort — the exact sequence that ended the
    // real incident with 18+ undelivered repeats.
    let texts = fixture_lines("text|");
    assert!(texts.len() >= 6, "fixture must carry the announcement loop");
    let mut ring = OutgoingTextRing::default();
    let mut saw_nudge = false;
    let mut saw_abort = false;
    for t in &texts {
        match ring.record_and_check(t) {
            TextLoopAction::Nudge => saw_nudge = true,
            TextLoopAction::Abort => {
                assert!(saw_nudge, "abort fired before the nudge did");
                saw_abort = true;
            }
            TextLoopAction::Continue => {
                assert!(!saw_nudge, "loop continued after the nudge");
            }
        }
    }
    assert!(saw_nudge, "the fixture never tripped the nudge");
    assert!(saw_abort, "the fixture never escalated to abort");
}
