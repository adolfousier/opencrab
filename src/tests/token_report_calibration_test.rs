//! Regression (#942): the ctx counter must track the provider's reported
//! prompt size on EVERY response, not only when the two disagree sharply.
//!
//! The write-back used to sit inside the `drift > 5000` branch that exists for
//! the over-reporting guard. A report agreeing within that threshold was never
//! adopted, so the count silently stayed on the local tiktoken estimate. The
//! displayed ctx then alternated between two measurement systems — provider
//! truth on big-drift turns, drifting estimate on the rest — and could fall
//! between turns with nothing removed from context.

use crate::brain::agent::service::tool_loop::{TokenReport, evaluate_token_report};

/// Tool schemas large enough that the implausibility guard is live.
const TOOL_TOKENS: usize = 4_000;

#[test]
fn a_report_close_to_the_estimate_is_still_adopted() {
    // The exact case that was dropped: the API and the estimate agree within
    // the 5000-token guard band, so nothing was written back.
    let local = 280_000;
    let reported = 282_000; // drift 2_000, well inside the old threshold
    assert_eq!(
        evaluate_token_report(local, TOOL_TOKENS, reported),
        TokenReport::Adopt(reported),
        "a plausible report must be adopted regardless of how small the drift is"
    );
}

#[test]
fn an_identical_report_is_adopted_rather_than_treated_as_nothing_to_do() {
    let local = 100_000;
    assert_eq!(
        evaluate_token_report(local, TOOL_TOKENS, local),
        TokenReport::Adopt(local),
        "agreement is the normal case, not a skip"
    );
}

#[test]
fn a_large_but_believable_report_is_adopted() {
    // Drift beyond the guard band, but nowhere near 2x the real content, so
    // this is ordinary growth and must be trusted.
    let local = 270_000;
    let reported = 282_000;
    assert_eq!(
        evaluate_token_report(local, TOOL_TOKENS, reported),
        TokenReport::Adopt(reported),
        "a big honest jump must still be adopted"
    );
}

#[test]
fn an_over_reporting_endpoint_is_still_rejected() {
    // The guard this threshold exists for: a proxy inflating every call.
    // Must survive the fix.
    let local = 10_000;
    let reported = (local + TOOL_TOKENS) * 3;
    assert_eq!(
        evaluate_token_report(local, TOOL_TOKENS, reported),
        TokenReport::RejectImplausible,
        "an endpoint reporting >2x the real content must not be trusted"
    );
}

#[test]
fn a_small_inflation_is_not_mistaken_for_over_reporting() {
    // Just over 2x the content but inside the drift band — the drift floor
    // must keep the rejection from firing on small absolute numbers, which is
    // what it was originally guarding.
    let local = 1_000;
    let reported = 3_000;
    assert_eq!(
        evaluate_token_report(local, TOOL_TOKENS, reported),
        TokenReport::Adopt(reported),
        "a small absolute disagreement is tokenizer variance, not inflation"
    );
}

#[test]
fn a_truncated_usage_block_is_ignored() {
    assert_eq!(
        evaluate_token_report(200_000, TOOL_TOKENS, 42),
        TokenReport::BelowSanityFloor,
        "a handful of tokens cannot be a real prompt"
    );
    assert_eq!(
        evaluate_token_report(200_000, TOOL_TOKENS, 0),
        TokenReport::BelowSanityFloor,
        "a missing usage block must not zero the counter"
    );
}

#[test]
fn a_collapse_to_a_fraction_of_the_estimate_is_ignored() {
    // Guards the counter against a report so much smaller that it would have
    // to mean the context was rebuilt, which this path cannot tell from a bad
    // report.
    assert_eq!(
        evaluate_token_report(200_000, TOOL_TOKENS, 10_000),
        TokenReport::ImplausibleDrop,
        "a drop to 5% of the estimate is not a believable prompt size"
    );
}

#[test]
fn a_moderate_decrease_is_adopted() {
    // Not every decrease is suspect: dropping a large tool result genuinely
    // shrinks the prompt, and refusing that would strand the counter high.
    let local = 200_000;
    let reported = 150_000;
    assert_eq!(
        evaluate_token_report(local, TOOL_TOKENS, reported),
        TokenReport::Adopt(reported),
        "a real shrink within the drop guard must be tracked"
    );
}

#[test]
fn successive_reports_track_the_provider_rather_than_wandering() {
    // The user-visible property: feed the sequence a growing conversation
    // produces and the adopted values must be exactly what was reported, so
    // the footer cannot fall while context is only being added.
    let reported = [274_000, 276_000, 280_000, 281_000, 284_000, 297_000];
    let mut count = 270_000;
    for r in reported {
        match evaluate_token_report(count, TOOL_TOKENS, r) {
            TokenReport::Adopt(actual) => {
                assert!(
                    actual >= count,
                    "adopted {actual} after {count}: a growing prompt must never be shown shrinking"
                );
                count = actual;
            }
            other => panic!("plausible report {r} was not adopted: {other:?}"),
        }
    }
    assert_eq!(
        count, 297_000,
        "the counter must end on the last reported value"
    );
}
