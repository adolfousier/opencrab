//! Fact-based detectors must be able to FIRE the self-heal, not merely gate it
//! (#1073).
//!
//! `phantom_eligible` and the nudge trigger are two separate conditions in
//! `tool_loop`. `claims_uncalled_commands` (#789) and `claims_unbacked_evidence`
//! (#785) appeared only in the first. A turn could therefore be ruled eligible
//! on fact-based evidence, fail every wording branch of the trigger, and ship
//! the fabrication with no correction and no log line — the detector ran, was
//! correct, and had no path to the nudge it was computed for.
//!
//! The shape that slipped through is a PAST-TENSE result claim: it reports
//! output from a command that never ran, carrying none of the signals the
//! wording detectors look for — no forward intent, no registered tool name, no
//! side-effect verb, not a bare completion, no media claim.
//!
//! These tests pin the asymmetry itself: the fact-based check must catch what
//! every wording check misses, and must stay quiet when the command really ran.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::brain::agent::service::phantom::{
    claims_unbacked_evidence, claims_unbacked_media_result, claims_unbacked_side_effects,
    claims_uncalled_commands, has_phantom_tool_intent, has_phantom_tool_intent_no_tools,
    is_bare_completion_only, mentions_registered_tool,
};

/// A past-tense claim about a command that never ran, phrased so no wording
/// heuristic has anything to match on. The `i ran` framing is what makes this
/// a claim of execution rather than a proposal.
const FABRICATED: &str =
    "I ran `git branch --contains abc1234` and main does not carry either fix.";

fn registered_tools() -> Vec<String> {
    ["bash", "read_file", "write_file", "grep", "telegram_send"]
        .iter()
        .map(|s| (*s).to_string())
        .collect()
}

#[test]
fn the_fact_based_check_catches_what_every_wording_check_misses() {
    // Zero tools ran, so nothing backs the claim.
    let executed: Vec<String> = vec![];

    // Fact-based: the named command appears in no tool input, so it did not
    // run. True as a matter of fact, not as a reading of the wording.
    assert_eq!(
        claims_uncalled_commands(FABRICATED, &executed),
        vec!["git branch --contains abc1234"],
        "fact-based detector must catch a command that never ran"
    );

    // Every wording branch of the trigger misses this text. That combination —
    // fact-based hit, wording-based miss — is exactly the gap #1073 closed:
    // before the fix these six were the ONLY things that could fire the nudge.
    assert!(
        !has_phantom_tool_intent_no_tools(FABRICATED),
        "past-tense claim carries no forward intent"
    );
    assert!(
        !has_phantom_tool_intent(FABRICATED),
        "strict intent detector finds no narrated plan"
    );
    assert!(
        !mentions_registered_tool(FABRICATED, &registered_tools()),
        "`git` is not a registered tool name (`bash` is)"
    );
    assert!(
        !claims_unbacked_side_effects(FABRICATED),
        "no ship/push/tag/changelog verbs"
    );
    assert!(
        !is_bare_completion_only(FABRICATED),
        "not a bare completion acknowledgement"
    );
    assert!(
        !claims_unbacked_media_result(FABRICATED),
        "no image/video claim"
    );
}

#[test]
fn the_same_claim_is_clean_when_the_command_really_ran() {
    // The guard must not fire when the turn actually executed the command it
    // reports on, otherwise honest reporting would be corrected as fabrication.
    let executed = vec![r#"{"command":"git branch --contains abc1234 --all"}"#.to_string()];
    assert!(
        claims_uncalled_commands(FABRICATED, &executed).is_empty(),
        "a command that really ran must not be flagged"
    );
}

#[test]
fn a_legitimate_completion_after_real_work_stays_quiet() {
    // Guards the e843f405 regression: phantom detection firing on genuine
    // completion acknowledgements after successful work. Neither fact-based
    // check may fire here — no command is named, and the quoted figure is
    // present in a real tool result.
    let text = "Committed the change. The suite reports 6549 passed, 0 failed.";
    let executed = vec![r#"{"command":"cargo test --all-features --lib"}"#.to_string()];
    let outputs = vec!["test result: ok. 6549 passed; 0 failed; 25 ignored".to_string()];

    assert!(
        claims_uncalled_commands(text, &executed).is_empty(),
        "no uncalled command is named"
    );
    assert!(
        !claims_unbacked_evidence(text, &outputs),
        "the quoted figure appears in a real tool result"
    );
}
