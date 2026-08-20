//! Forward intent whose verb nobody enumerated, and intent that lands in the
//! turn's tail.
//!
//! Both texts here are real turns that ended with work promised and not done,
//! captured from a running session. The phrase list is an allowlist of verb
//! pairings, so `let me execute` was absent from all 701 entries and the turn
//! closed clean; and the post-success gate read only the opening prose, so a
//! promise made after the tool results was never examined.

use crate::brain::agent::service::phantom::{
    has_forward_intent_post_success, has_phantom_tool_intent_no_tools,
};

/// The turn ran no tools and ended on a promise to run the whole flow.
const UNENUMERATED_VERB: &str = "You're right. The review gate means: do all the work \
(Steps 1-8), present ONE draft, wait for approval. Not stop after every git command. \
Let me execute the full flow now and only stop when I have the draft ready.";

/// The turn ran 16 tools, then promised two more steps in its closing line.
const PROMISE_IN_TAIL: &str = "No worries! \n\nNow, back to the plan: task 4 is up, \
verify and diff, then commit. Let me run the gate checks.\n\n\
| step | result |\n| --- | --- |\n| analyze | 2 info |\n\n\
Good - only 2 pre-existing info-level warnings (unnecessary imports), no errors from \
our changes. Let me run tests and check the diff.";

#[test]
fn a_verb_missing_from_the_phrase_list_is_still_intent() {
    assert!(
        has_phantom_tool_intent_no_tools(UNENUMERATED_VERB),
        "'let me execute' promises work as plainly as 'let me run'; the phrase list \
         simply never listed the verb"
    );
}

#[test]
fn a_promise_after_the_tool_results_is_not_a_completion_ack() {
    assert!(
        has_forward_intent_post_success(PROMISE_IN_TAIL),
        "the promise sits in the closing line, past the table, where a lead-only \
         window never looked"
    );
}

#[test]
fn an_enumerated_verb_still_matches() {
    assert!(has_phantom_tool_intent_no_tools(
        "Let me run the gate checks before we continue with anything else."
    ));
}

#[test]
fn offering_to_answer_is_not_a_promise_of_work() {
    // `let me know` addresses the reader. Matching the construction without
    // reading the verb would turn every closing courtesy into a phantom.
    assert!(
        !has_phantom_tool_intent_no_tools(
            "That is everything from the audit, and the numbers all check out. \
             Let me know if you want the full breakdown."
        ),
        "an offer to answer is speech, not work"
    );
}

#[test]
fn a_genuine_completion_ack_still_ends_the_turn() {
    assert!(
        !has_forward_intent_post_success(
            "Done. Committed as 4ac898a2 and pushed to main, and the working tree is clean."
        ),
        "a turn that finished its work must not be dragged back for another lap"
    );
}
