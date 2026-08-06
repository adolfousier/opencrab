//! A continuation extends the partial, it never replaces it (#859).
//!
//! `final_text` is built from the LAST response only, on the assumption that
//! earlier text already reached the user as `IntermediateText`. That is true in
//! the TUI and stopped being true for Telegram once intermediates were gated to
//! deliverable rich reports (#838): a plain-prose partial is emitted, dropped
//! by the gate, and the continuation alone becomes the answer.
//!
//! Observed: a 551-token answer was replaced by a 60-character provider
//! refusal. The refusal was the tail of the partial AND the entirety of the
//! continuation, so the user received only the refusal and none of the answer.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::brain::agent::service::truncation::{Continuation, join_continuation};

/// The joined text, whatever the outcome — most of these cases assert on the
/// content, and the outcome itself is covered by its own tests below.
fn joined_text(partial: &str, continuation: &str) -> String {
    match join_continuation(partial, continuation) {
        Continuation::Extended(s) | Continuation::Echoed(s) => s,
    }
}

#[test]
fn the_observed_case_keeps_the_answer() {
    // The continuation echoed the tail it was asked to continue from. Taking
    // the continuation alone discarded everything before it.
    let partial = "Ainda nao. Estado real: o motor existe mas nao esta ligado. \
                   The request was rejected because it was considered high risk";
    let continuation = "The request was rejected because it was considered high risk";
    let joined = joined_text(partial, continuation);
    assert!(
        joined.contains("Estado real"),
        "the answer was dropped again: {joined}"
    );
    assert_eq!(joined, partial, "nothing new to add, so keep the partial");
}

#[test]
fn a_genuine_continuation_is_appended() {
    let partial = "The three failing tests are in the parser, the";
    let continuation = " renderer and the scheduler.";
    let joined = joined_text(partial, continuation);
    assert!(joined.starts_with("The three failing tests"));
    assert!(joined.ends_with("scheduler."));
}

#[test]
fn a_mid_word_cut_is_not_corrupted_by_a_separator() {
    // The detector fires on an alphanumeric last character, which includes
    // mid-word. Inserting a space there would corrupt the word.
    let joined = joined_text("the scheduler was reconfig", "ured last night");
    assert!(
        joined.contains("reconfigured"),
        "the word was split: {joined}"
    );
}

#[test]
fn a_restated_continuation_does_not_duplicate_the_partial() {
    // Some models restart and reproduce everything. Concatenating would show
    // the whole answer twice.
    let partial = "Here are the three findings";
    let continuation = "Here are the three findings, listed in full below.";
    assert_eq!(joined_text(partial, continuation), continuation);
}

#[test]
fn an_empty_continuation_keeps_the_partial() {
    // The continuation request can come back with nothing. Losing the partial
    // then would be the original bug in its purest form.
    let partial = "A complete enough answer that simply lacked a full stop";
    assert_eq!(joined_text(partial, ""), partial);
    assert_eq!(joined_text(partial, "   \n "), partial);
}

#[test]
fn an_empty_partial_keeps_the_continuation() {
    assert_eq!(joined_text("", "the answer"), "the answer");
    assert_eq!(joined_text("   ", "the answer"), "the answer");
}

#[test]
fn both_empty_yields_empty() {
    assert_eq!(joined_text("", ""), "");
}

#[test]
fn a_clause_boundary_gets_a_separator() {
    // Two clauses run together are worse to read than one stray space, so a
    // non-alphanumeric end takes the separator.
    let joined = joined_text("First, the parser fails;", "second, the renderer does too.");
    assert!(joined.contains("; second,"), "{joined}");
}

#[test]
fn existing_whitespace_is_not_doubled() {
    let joined = joined_text("the list is: ", " one, two, three");
    assert!(
        !joined.contains("  "),
        "double space introduced: {joined:?}"
    );
}

// ── Outcome, not just text (#956) ────────────────────────────────────────────
//
// A continuation that recovered nothing used to be indistinguishable from one
// that worked: both returned a String. So a turn cut off mid-sentence was
// delivered as a finished answer — the reported case ended on a colon,
// "Closing #947 with the full remarks:", with nothing after it.

#[test]
fn an_echoed_continuation_is_reported_as_recovering_nothing() {
    // The observed failure: 452-char partial, 35-char continuation, 452 chars
    // out. The model echoed the tail it was asked to continue from.
    let partial = "Commit landed, tree clean, not pushed. Closing #947 with the full remarks:";
    let echo = "Closing #947 with the full remarks:";
    assert_eq!(
        join_continuation(partial, echo),
        Continuation::Echoed(partial.to_string()),
        "an echo must be reported as a failure, not returned as a finished answer"
    );
}

#[test]
fn an_empty_continuation_is_reported_as_recovering_nothing() {
    // Nothing came back, so nothing was recovered — same outcome as an echo.
    let partial = "The answer begins here and stops";
    assert_eq!(
        join_continuation(partial, ""),
        Continuation::Echoed(partial.to_string())
    );
    assert_eq!(
        join_continuation(partial, "  \n "),
        Continuation::Echoed(partial.to_string())
    );
}

#[test]
fn a_genuine_continuation_is_reported_as_extended() {
    match join_continuation("The three failing tests", " are all in the parser.") {
        Continuation::Extended(text) => {
            assert!(text.contains("parser"), "got: {text}");
        }
        other => panic!("a real continuation must extend: {other:?}"),
    }
}

#[test]
fn a_full_restatement_counts_as_progress_not_an_echo() {
    // The model restarted and reproduced the whole answer. That carries the
    // answer forward even though it repeats the partial, so it must NOT be
    // marked incomplete.
    let partial = "The parser fails on";
    let restated = "The parser fails on nested arrays, and the fix is one line.";
    assert_eq!(
        join_continuation(partial, restated),
        Continuation::Extended(restated.to_string()),
        "a restatement that adds content is progress, not an echo"
    );
}

#[test]
fn an_empty_partial_takes_the_continuation_as_progress() {
    assert_eq!(
        join_continuation("", "the whole answer"),
        Continuation::Extended("the whole answer".to_string())
    );
}

#[test]
fn the_incomplete_marker_says_what_happened_and_what_to_do() {
    use crate::brain::agent::service::truncation::INCOMPLETE_MARKER;
    let m = INCOMPLETE_MARKER.to_lowercase();
    assert!(
        m.contains("cut off"),
        "must name the problem: {INCOMPLETE_MARKER}"
    );
    assert!(
        m.contains("ask"),
        "a marker the user cannot act on is just noise: {INCOMPLETE_MARKER}"
    );
}
