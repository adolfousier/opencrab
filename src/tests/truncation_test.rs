//! Truncation detection and one-shot continuation: usage gap, structural shapes (#36), join semantics (#859/#956).

use crate::brain::agent::service::phantom::looks_truncated_mid_sentence;
use crate::brain::agent::service::truncation::*;
use crate::brain::provider::TokenUsage;
fn usage(output: u32, reasoning: u32) -> TokenUsage {
    TokenUsage {
        output_tokens: output,
        reasoning_tokens: reasoning,
        ..Default::default()
    }
}

// ── has_usage_gap ──────────────────────────────────────────────

#[test]
fn usage_gap_incident_numbers() {
    // The 2026-08-28 #36 incident: 927 completion tokens billed, 457
    // of them reasoning, but only 316 chars (~80 tokens) arrived.
    let text = "x".repeat(316);
    assert!(has_usage_gap(&text, &usage(927, 457)));
}

#[test]
fn usage_gap_healthy_text() {
    // ~4 chars per visible token is normal English prose — no gap.
    let text = "The deployment completed without issues.".repeat(5); // 190 chars
    assert!(!has_usage_gap(&text, &usage(50, 0)));
}

#[test]
fn usage_gap_reasoning_heavy_no_false_positive() {
    // Thinking-heavy turn: almost all output is reasoning, the small
    // visible remainder matches the text. Must NOT flag.
    let text = "x".repeat(300);
    assert!(!has_usage_gap(&text, &usage(5000, 4900)));
}

#[test]
fn usage_gap_no_usage_no_signal() {
    let text = "x".repeat(10);
    assert!(!has_usage_gap(&text, &usage(0, 0)));
    // All output is reasoning — nothing visible was billed.
    assert!(!has_usage_gap(&text, &usage(100, 100)));
}

// ── structural shapes added for #36 ────────────────────────────

#[test]
fn backtick_parity_incident_shape() {
    // Stream died on an OPENING backtick: exactly one backtick in the
    // whole reply (odd parity) — the #36 shape, previously read as
    // complete because a lone backtick matched no cut character.
    let truncated = "You can find the full path in the config file under `";
    assert!(looks_truncated_mid_sentence(truncated));
}

#[test]
fn backtick_closed_inline_code_clean() {
    // Balanced backticks: the last one CLOSES inline code — complete.
    let complete = "To check the status of the repository you run `git status`";
    assert!(!looks_truncated_mid_sentence(complete));
}

#[test]
fn unclosed_fence_truncated() {
    let text = "Here is the script you asked for:\n\n```bash\necho hello\nls -la";
    assert!(looks_truncated_mid_sentence(text));
}

#[test]
fn closed_fence_clean() {
    let text = "Here is the script you asked for:\n\n```bash\necho hello\n```\nRun it and tell me what it prints.";
    assert!(!looks_truncated_mid_sentence(text));
}

#[test]
fn em_dash_tail_truncated() {
    let text = "The migration finished cleanly and the only remaining step is the cutover —";
    assert!(looks_truncated_mid_sentence(text));
}

#[test]
fn healthy_period_clean() {
    let text = "The migration finished cleanly and nothing else needs to be done.";
    assert!(!looks_truncated_mid_sentence(text));
}

// ── join_continuation semantics (#859/#956 regressions) ────────

#[test]
fn join_echoed_tail_recovers_nothing() {
    let partial = "The answer is forty two tokens and the reason is";
    let cont = "and the reason is";
    assert_eq!(
        join_continuation(partial, cont),
        Continuation::Echoed(partial.to_string())
    );
}

#[test]
fn join_genuine_continuation_extends() {
    let partial = "The answer is forty two tokens and the reason is";
    let cont = "that the model counted every token twice.";
    match join_continuation(partial, cont) {
        Continuation::Extended(j) => {
            assert!(j.starts_with(partial));
            assert!(j.ends_with("twice."));
        }
        other => panic!("expected Extended, got {other:?}"),
    }
}

#[test]
fn join_restart_reproducing_partial_is_progress() {
    let partial = "The answer is forty";
    let cont = "The answer is forty two tokens.";
    assert_eq!(
        join_continuation(partial, cont),
        Continuation::Extended(cont.to_string())
    );
}
