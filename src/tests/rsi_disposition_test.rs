//! Capability gaps must be recoverable from the opportunity text (#842).
//!
//! The detectors name `rsi_propose` and the kind in the text they build, then
//! that disposition was discarded: the agent received a flat bullet list and a
//! closing "apply improvements". Across 116 opportunity-bearing cycles it made
//! zero `rsi_propose` calls, answering every capability gap with a brain-file
//! rule instead. No rule closes a missing capability, so each gap was
//! re-detected and re-answered the next cycle.
//!
//! Fixtures reproduce the detectors' real phrasing with synthetic subjects and
//! carry no user identifiers.

use crate::brain::rsi_disposition::{
    ProposalKind, capability_count, required_actions_block, required_kind,
};

/// The promotion detector's wording, which offers a choice of two kinds.
const PROMOTION: &str = "Bash subsystem 'example' has 288 successful invocations in the window. \
     Promotion candidate: file a tool (rsi_propose kind=tool) for the recurring command shape, \
     or a skill (kind=skill) for the workflow it codifies.";

const COMMAND_PATTERN: &str = "User request pattern typed 4 times - candidate for a slash \
     command (rsi_propose kind=command). File a concise /<name> whose prompt captures the \
     recurring intent.";

const SKILL_SEQUENCE: &str = "Tool sequence 'a -> b -> c' ran in 6 distinct sessions - candidate \
     for a skill (rsi_propose kind=skill). File a SKILL.md codifying the workflow.";

const GUIDANCE: &str = "50 user corrections recorded. Review patterns and update brain files.";

#[test]
fn each_detector_shape_is_classified() {
    assert_eq!(required_kind(PROMOTION), Some(ProposalKind::Tool));
    assert_eq!(required_kind(COMMAND_PATTERN), Some(ProposalKind::Command));
    assert_eq!(required_kind(SKILL_SEQUENCE), Some(ProposalKind::Skill));
}

#[test]
fn when_two_kinds_are_offered_the_first_wins() {
    // The promotion detector names tool then skill. Picking the later one
    // would file the wrong shape for a parameterised command.
    assert_eq!(required_kind(PROMOTION), Some(ProposalKind::Tool));
}

#[test]
fn a_guidance_opportunity_is_not_a_capability_gap() {
    // Corrections and provider errors ARE brain-file work. Misclassifying
    // them would push noise into the inbox, which is as useless as an empty
    // one.
    assert_eq!(required_kind(GUIDANCE), None);
    assert_eq!(
        required_kind("20 provider errors recorded. Recent errors: ..."),
        None
    );
}

#[test]
fn merely_naming_the_tool_is_not_enough() {
    // Without a kind marker there is nothing to file, so it stays guidance
    // rather than becoming a proposal of unknown shape.
    assert_eq!(
        required_kind("Consider whether rsi_propose applies here."),
        None
    );
}

#[test]
fn counting_ignores_guidance() {
    let opps = vec![
        GUIDANCE.to_string(),
        PROMOTION.to_string(),
        SKILL_SEQUENCE.to_string(),
    ];
    assert_eq!(capability_count(&opps), 2);
}

#[test]
fn a_guidance_only_cycle_gets_no_extra_block() {
    // The prompt keeps its original shape when there is nothing to propose.
    let opps = vec![GUIDANCE.to_string()];
    assert!(required_actions_block(&opps).is_empty());
}

#[test]
fn an_empty_cycle_gets_no_extra_block() {
    assert!(required_actions_block(&[]).is_empty());
}

#[test]
fn the_block_names_the_tool_the_kind_and_the_position() {
    let opps = vec![
        GUIDANCE.to_string(),
        PROMOTION.to_string(),
        COMMAND_PATTERN.to_string(),
    ];
    let block = required_actions_block(&opps);
    // Positions are 1-based and must match the numbered opportunity list, or
    // the instruction points at the wrong finding.
    assert!(block.contains("Opportunity 2: call rsi_propose with kind=tool"));
    assert!(block.contains("Opportunity 3: call rsi_propose with kind=command"));
    assert!(
        !block.contains("Opportunity 1:"),
        "guidance must not be listed as requiring a proposal"
    );
}

#[test]
fn the_block_rules_out_the_answer_that_was_being_given() {
    let block = required_actions_block(&[PROMOTION.to_string()]);
    assert!(
        block.contains("`self_improve` does NOT answer these"),
        "the block must name the wrong answer explicitly"
    );
}

#[test]
fn the_block_states_the_expected_call_count() {
    let opps = vec![
        PROMOTION.to_string(),
        COMMAND_PATTERN.to_string(),
        SKILL_SEQUENCE.to_string(),
    ];
    let block = required_actions_block(&opps);
    assert!(block.contains("That is 3 required rsi_propose call(s)"));
}

#[test]
fn skipping_is_allowed_but_must_be_stated() {
    // A hard "you must file all of them" would push duplicates into the
    // inbox. The requirement is that a skip is accounted for, not forbidden.
    let block = required_actions_block(&[PROMOTION.to_string()]);
    assert!(block.contains("Do not silently skip it."));
}

#[test]
fn classification_is_case_insensitive() {
    assert_eq!(
        required_kind("Promotion candidate: RSI_PROPOSE KIND=TOOL for the shape."),
        Some(ProposalKind::Tool)
    );
}
