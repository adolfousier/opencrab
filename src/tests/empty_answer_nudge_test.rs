//! The empty-answer nudge must not be escapable by going quiet (#978).
//!
//! Observed failure: a turn nudged once, the retry came back completely silent,
//! and the turn ended delivering nothing. The channel showed `nudge 1/5`, then a
//! `Finished` card with no answer and no tool calls, repeatedly. The counter
//! never reached 2/5, so the retry budget was never exhausted and the fallback
//! chain was never walked.
//!
//! Cause: the trigger required an empty answer AND 40+ characters of reasoning,
//! so the WORST response shape (no answer, no reasoning) was the one shape that
//! received no retry.

use crate::brain::agent::service::helpers::{empty_reasoning_nudge, should_nudge_empty_answer};

#[test]
fn an_empty_answer_with_no_reasoning_is_nudged() {
    // The regression. This shape used to fall straight through and end the turn.
    assert!(should_nudge_empty_answer(1, false, ""));
}

#[test]
fn whitespace_only_counts_as_empty() {
    assert!(should_nudge_empty_answer(1, false, "   \n\t  "));
}

#[test]
fn a_real_answer_is_never_nudged() {
    assert!(!should_nudge_empty_answer(1, false, "Here is the answer."));
    // A single character is still an answer.
    assert!(!should_nudge_empty_answer(1, false, "x"));
}

#[test]
fn cli_providers_are_excluded() {
    // They run their own loop internally, so an empty outer response is normal.
    assert!(!should_nudge_empty_answer(1, true, ""));
}

#[test]
fn the_opening_iteration_is_excluded() {
    // Iteration 0 has not had a chance to act on anything yet.
    assert!(!should_nudge_empty_answer(0, false, ""));
}

#[test]
fn it_keeps_firing_across_every_iteration_of_a_turn() {
    // This is what lets the counter climb 1/5 -> 5/5 and finally exhaust the
    // budget, which is the only path to the fallback chain. Under the old
    // condition a silent retry broke this chain at the first step.
    for iteration in 1..=5 {
        assert!(
            should_nudge_empty_answer(iteration, false, ""),
            "iteration {iteration} must still nudge"
        );
    }
}

#[test]
fn the_nudge_text_sharpens_as_attempts_climb() {
    // Escalation is only meaningful if later attempts differ; five identical
    // nudges would just be the same failed ask five times.
    let first = empty_reasoning_nudge(false, 1);
    let last = empty_reasoning_nudge(false, 5);
    assert_ne!(first, last);
}

#[test]
fn the_nudge_differs_by_whether_a_tool_has_run() {
    // With no tool executed the model still needs to CALL one, so the nudge must
    // not tell it the results so far are sufficient (#692 regression).
    assert_ne!(
        empty_reasoning_nudge(true, 1),
        empty_reasoning_nudge(false, 1)
    );
}
