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

// ── Self-update claims (#780) ───────────────────────────────────────────────
// "Evolved from X to Y" with zero tool calls told the user their binary had
// replaced itself when it had not. Worse than a fabricated issue close: they
// then believe they are running a version they are not, and any bug report
// against it is unfalsifiable.

#[test]
fn a_fabricated_self_update_is_flagged() {
    assert!(has_phantom_tool_intent_no_tools(
        "Evolved from 0.3.74 to 0.3.75. The binary is now current."
    ));
}

#[test]
fn the_other_lifecycle_claims_are_flagged_too() {
    for claim in [
        "Upgraded the binary to the latest release.",
        "Restarted the service so the new config applies.",
        "Rebuilt it from source and the warnings are gone.",
    ] {
        assert!(
            has_phantom_tool_intent_no_tools(claim),
            "should flag as a completion claim: {claim}"
        );
    }
}

#[test]
fn describing_a_version_change_is_not_a_claim() {
    // Passive and descriptive prose about state must stay untouched, or
    // explaining what evolve DOES becomes impossible.
    assert!(!has_phantom_tool_intent_no_tools(
        "The binary is evolved by the release job, not by anything you run locally."
    ));
}

// ── Buried announcements (#783) ─────────────────────────────────────────────
// Every rule matched the lead only: the first 7 lines, or up to the first list
// item. That held while a turn opened with its intent, and stopped holding once
// reasoning occupied the lead — the announcement slid past the window and only
// deliberation was examined. Zero-tool turns then went unenforced.
//
// Fixtures are synthetic and carry no user identifiers.

#[test]
fn an_announcement_after_a_numbered_plan_is_still_caught() {
    // prose_lead_in stops dead at "1. ", so everything below it used to be
    // invisible — which is exactly where the announcement lands.
    let text = "The request is a scoring diagnosis with five deliverables.\n\
                Time to execute.\n\
                Plan:\n\
                1. Read the service file\n\
                2. Understand the axes\n\
                3. Query the database\n\n\
                Let me start. First locate and read the service file.";
    assert!(has_phantom_tool_intent_no_tools(text));
}

#[test]
fn an_announcement_below_the_seven_line_window_is_still_caught() {
    let text = "Line one of deliberation.\nLine two.\nLine three.\nLine four.\n\
                Line five.\nLine six.\nLine seven.\nLine eight.\n\
                On it. Starting the diagnosis now.";
    assert!(has_phantom_tool_intent_no_tools(text));
}

#[test]
fn a_lead_announcement_still_fires_as_before() {
    // The window that already worked must keep working.
    assert!(has_phantom_tool_intent_no_tools(
        "Let me read the config file and see what it says about the timeout."
    ));
}

#[test]
fn a_closing_courtesy_is_not_an_announcement() {
    // "let me know" is the obvious tail false positive. Every pattern is a
    // specific verb phrase, so it must not match.
    assert!(!has_phantom_tool_intent_no_tools(
        "The timeout is set to 30 seconds in the config, and the retry budget \
         is three attempts. Let me know if you want either changed."
    ));
}

#[test]
fn a_table_at_the_end_is_not_read_as_a_parting_announcement() {
    let text = "Here is the comparison you asked for.\n\n\
                | axis | before | after |\n\
                |---|---|---|\n\
                | score | 3 | 7 |";
    assert!(!has_phantom_tool_intent_no_tools(text));
}
