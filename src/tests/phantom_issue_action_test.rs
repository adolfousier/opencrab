//! Issue-tracker actions count as completion claims.
//!
//! A turn with ZERO tool calls announced `"Closed #767 as completed with
//! remarks."` and the phantom detector let it through. The sentence cleared
//! every structural gate: prose lead present, well under the 80-char sentence
//! limit, active rather than passive. It failed on vocabulary alone, because
//! `action_verbs` held only deploy-pipeline words and no form of close,
//! comment, file, open or assign.
//!
//! That gate is what enables the whole self-heal path, so a miss there means
//! no retry, no provider nudge, no narration strip. The fabrication reached
//! the user as fact.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::brain::agent::service::phantom::has_phantom_tool_intent_no_tools;

#[test]
fn a_fabricated_issue_close_is_flagged() {
    // The reported text, shortened only of its trailing detail.
    assert!(has_phantom_tool_intent_no_tools(
        "Closed #767 as completed with remarks. The comment credits the original author."
    ));
}

#[test]
fn the_other_collaboration_actions_are_flagged_too() {
    for claim in [
        "Commented on the issue with the findings.",
        "Filed the follow-up issue for the detector gap.",
        "Reopened it so the regression stays tracked.",
        "Assigned it to the maintainer for review.",
        "Approved the pull request after reading the diff.",
    ] {
        assert!(
            has_phantom_tool_intent_no_tools(claim),
            "should flag as a completion claim: {claim}"
        );
    }
}

#[test]
fn passive_description_is_still_not_a_claim() {
    // The passive guard must survive the new verbs: describing state is not
    // announcing an action. "The issue is closed" is prose about the world.
    assert!(!has_phantom_tool_intent_no_tools(
        "The issue is closed, which is why the label no longer applies to it."
    ));
    assert!(!has_phantom_tool_intent_no_tools(
        "That pull request was merged upstream long before this branch existed."
    ));
}

#[test]
fn a_question_about_closing_is_not_a_claim() {
    // Asking is the opposite of asserting; flagging it would suppress a
    // legitimate clarification.
    assert!(!has_phantom_tool_intent_no_tools(
        "Do you want this closed with remarks, or left open until the release?"
    ));
}

#[test]
fn the_deploy_verbs_still_work() {
    // Guard against the additions displacing what already worked.
    assert!(has_phantom_tool_intent_no_tools(
        "Pushed the branch and deployed it to staging for you."
    ));
}
