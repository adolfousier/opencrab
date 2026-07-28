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

use crate::brain::agent::service::truncation::join_continuation;

#[test]
fn the_observed_case_keeps_the_answer() {
    // The continuation echoed the tail it was asked to continue from. Taking
    // the continuation alone discarded everything before it.
    let partial = "Ainda nao. Estado real: o motor existe mas nao esta ligado. \
                   The request was rejected because it was considered high risk";
    let continuation = "The request was rejected because it was considered high risk";
    let joined = join_continuation(partial, continuation);
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
    let joined = join_continuation(partial, continuation);
    assert!(joined.starts_with("The three failing tests"));
    assert!(joined.ends_with("scheduler."));
}

#[test]
fn a_mid_word_cut_is_not_corrupted_by_a_separator() {
    // The detector fires on an alphanumeric last character, which includes
    // mid-word. Inserting a space there would corrupt the word.
    let joined = join_continuation("the scheduler was reconfig", "ured last night");
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
    assert_eq!(join_continuation(partial, continuation), continuation);
}

#[test]
fn an_empty_continuation_keeps_the_partial() {
    // The continuation request can come back with nothing. Losing the partial
    // then would be the original bug in its purest form.
    let partial = "A complete enough answer that simply lacked a full stop";
    assert_eq!(join_continuation(partial, ""), partial);
    assert_eq!(join_continuation(partial, "   \n "), partial);
}

#[test]
fn an_empty_partial_keeps_the_continuation() {
    assert_eq!(join_continuation("", "the answer"), "the answer");
    assert_eq!(join_continuation("   ", "the answer"), "the answer");
}

#[test]
fn both_empty_yields_empty() {
    assert_eq!(join_continuation("", ""), "");
}

#[test]
fn a_clause_boundary_gets_a_separator() {
    // Two clauses run together are worse to read than one stray space, so a
    // non-alphanumeric end takes the separator.
    let joined = join_continuation("First, the parser fails;", "second, the renderer does too.");
    assert!(joined.contains("; second,"), "{joined}");
}

#[test]
fn existing_whitespace_is_not_doubled() {
    let joined = join_continuation("the list is: ", " one, two, three");
    assert!(
        !joined.contains("  "),
        "double space introduced: {joined:?}"
    );
}
