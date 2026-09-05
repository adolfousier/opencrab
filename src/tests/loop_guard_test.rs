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
use crate::brain::agent::service::helpers::{normalize_loop_text, normalized_call_signature};
use serde_json::json;

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
    // The reported log was Russian; the guard must normalize it cleanly.
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

// ---- normalized_call_signature (#961) ----

#[test]
fn counter_variant_tool_search_queries_collide() {
    // The #961 DeepSeek v4 flash pattern: tool_search re-issued with only a
    // trailing counter moving between attempts must collapse to the SAME
    // near-match signature so the generalized tool-layer guard can count it.
    let a = normalized_call_signature(
        "tool_search",
        &json!({"query": "send a document file to telegram"}),
    );
    let b = normalized_call_signature(
        "tool_search",
        &json!({"query": "send a document file to telegram 2"}),
    );
    assert_eq!(a, b);
}

#[test]
fn signature_collapses_counters_punctuation_and_case() {
    // Regression guard for the #957 bash path under the generalized helper.
    let a = normalized_call_signature("bash", &json!({"command": "echo \"Attempt 1 of 6\""}));
    let b = normalized_call_signature("bash", &json!({"command": "echo attempt 2 of 6"}));
    assert_eq!(a, b);
}

#[test]
fn different_tool_search_queries_stay_apart() {
    let a = normalized_call_signature(
        "tool_search",
        &json!({"query": "send a document file to telegram"}),
    );
    let b = normalized_call_signature("tool_search", &json!({"query": "schedule a cron job"}));
    assert_ne!(a, b);
}

#[test]
fn signature_is_namespaced_by_tool_name() {
    // Same args under different tool names must not collide.
    let a = normalized_call_signature("tool_search", &json!({"query": "send"}));
    let b = normalized_call_signature("telegram_send", &json!({"query": "send"}));
    assert_ne!(a, b);
}

#[test]
fn read_file_chunk_offsets_stay_apart() {
    // #82: numeric argument values are preserved exactly, so chunked reads
    // differing only in start_line no longer collapse — the tool loop no
    // longer needs its read_file exclusion, and genuine read loops (same
    // path, same range) still collide.
    let a = normalized_call_signature(
        "read_file",
        &json!({"path": "src/main.rs", "start_line": 100, "line_count": 50}),
    );
    let b = normalized_call_signature(
        "read_file",
        &json!({"path": "src/main.rs", "start_line": 150, "line_count": 50}),
    );
    assert_ne!(a, b);

    // Identical chunked reads (the stuck-loop shape) still collide.
    let c = normalized_call_signature(
        "read_file",
        &json!({"path": "src/main.rs", "start_line": 100, "line_count": 50}),
    );
    assert_eq!(a, c);
}

#[test]
fn plan_checklist_progression_stays_apart() {
    // #82 real-world false positive (2026-09-02): completing plan checklist
    // tasks differs ONLY in task_order — digit-stripping collapsed the
    // progression into one signature, nudging and then breaking a session
    // mid-checklist. Numeric fields must keep the calls distinct, while a
    // literally identical stuck plan call still collides.
    let done = |n: i64| {
        normalized_call_signature("plan", &json!({"operation": "complete", "task_order": n}))
    };
    assert_ne!(done(1), done(2));
    assert_ne!(done(2), done(3));
    assert_eq!(done(3), done(3));
}

#[test]
fn numeric_and_bool_fields_are_order_independent_and_exact() {
    // Numeric/bool params are kept exactly and sorted by key, so argument
    // insertion order never changes the signature (#82).
    let a = normalized_call_signature("bash", &json!({"command": "sleep 5", "timeout_secs": 120}));
    let b = normalized_call_signature("bash", &json!({"timeout_secs": 120, "command": "sleep 5"}));
    assert_eq!(a, b);
    let c = normalized_call_signature("bash", &json!({"command": "sleep 5", "timeout_secs": 60}));
    assert_ne!(a, c);
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
    // The ring caps at 8 (#961): fill it with distinct texts first, then
    // start the loop. Old distinct entries fall out and the
    // near-duplicates still reach the trip threshold.
    let mut ring = OutgoingTextRing::default();
    for t in [
        "First unrelated answer about the config audit",
        "Second unrelated answer with the benchmark numbers",
        "Third unrelated answer summarizing the migration",
        "Fourth unrelated answer about the flaky test fix",
        "Fifth unrelated answer closing out the review",
        "Sixth unrelated answer about the onboarding flow",
        "Seventh unrelated answer covering the rate limits",
        "Eighth unrelated answer wrapping up the retrospective",
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

// ---- #961 regression: DeepSeek v4 flash zip-send loop ----

#[test]
fn subset_announcement_counts_as_near_duplicate() {
    // The overlap-coefficient clause (#961): a short reworded
    // announcement whose words are almost all contained in a longer one
    // counts as a near-duplicate even though Jaccard is dragged down by
    // the length difference.
    assert!(near_duplicate(
        "Sending the zip to this thread now:",
        "Sending the zip:"
    ));
}

#[test]
fn overlap_clause_spares_unrelated_texts() {
    assert!(!near_duplicate(
        "Sending the zip to this thread now:",
        "Running the test suite again"
    ));
}

const ZIP_SEND_FIXTURE: &str = include_str!("fixtures/zip_send_announcement_loop.txt");

fn zip_send_fixture_lines(prefix: &str) -> Vec<String> {
    ZIP_SEND_FIXTURE
        .lines()
        .filter(|l| !l.trim_start().starts_with('#') && l.starts_with(prefix))
        .map(|l| l[prefix.len()..].to_string())
        .collect()
}

#[test]
fn deepseek_zip_send_announcements_nudge_then_abort() {
    // The text layer against the real #961 shape: the eight reworded
    // "sending now" announcements from the DeepSeek v4 flash zip-send
    // loop. With the overlap coefficient and the cap-8 ring the trip
    // sequence is exactly three cleans, nudge at the fourth, three more
    // cleans, abort at the eighth.
    let texts = zip_send_fixture_lines("text|");
    assert_eq!(texts.len(), 8, "fixture must carry all eight announcements");
    let mut ring = OutgoingTextRing::default();
    let expected = [
        TextLoopAction::Continue,
        TextLoopAction::Continue,
        TextLoopAction::Continue,
        TextLoopAction::Nudge,
        TextLoopAction::Continue,
        TextLoopAction::Continue,
        TextLoopAction::Continue,
        TextLoopAction::Abort,
    ];
    for (t, want) in texts.iter().zip(&expected) {
        assert_eq!(ring.record_and_check(t), *want, "text: {t}");
    }
}

#[test]
fn deepseek_zip_send_tool_queries_are_reworded_not_identical() {
    // Documents the tool-layer gap from #961: the re-activation queries
    // were reworded every attempt, so none are identical even after
    // normalization — the exact-match guards never saw a repeat at all.
    // (The generalized near-match catches counter/punctuation variants;
    // these deep rewordings are the text layer's job.)
    let queries = zip_send_fixture_lines("tool_search|");
    assert!(
        queries.len() >= 7,
        "fixture must carry the re-activation spiral"
    );
    let mut seen = std::collections::HashSet::new();
    for q in &queries {
        let n = normalize_loop_text(q);
        assert!(
            !seen.contains(&n),
            "query repeat would invalidate the #961 shape: {n}"
        );
        seen.insert(n);
    }
}
