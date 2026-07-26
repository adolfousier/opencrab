//! The RSI prompt must route capability gaps to a proposal (#811).
//!
//! `rsi_propose` was registered and documented, but the word "propose" appeared
//! nowhere in the agent's own workflow, so it was never called and the
//! proposal inbox stayed empty.
//!
//! Worse, the triage step correctly identified capability gaps, the exact case
//! proposals exist for, and then told the agent to drop them. A system that can
//! only write rules recognises every capability gap, categorises it, and
//! discards it.
//!
//! These pin the routing so it cannot be edited away by a later prompt
//! compression pass.

use crate::brain::rsi::RSI_AGENT_PROMPT;

#[test]
fn the_workflow_mentions_proposing() {
    // A tool the workflow never names is a tool that does not get used.
    assert!(
        RSI_AGENT_PROMPT.contains("rsi_propose"),
        "the prompt must name the proposal tool in its own workflow"
    );
}

#[test]
fn a_capability_gap_is_not_discarded() {
    // The old triage ended "a rule won't fix it and only adds noise — leave
    // it", which threw away precisely the findings proposals are for.
    assert!(
        !RSI_AGENT_PROMPT.contains("only adds noise — leave it"),
        "a capability gap must route to a proposal, not be dropped"
    );
    assert!(
        RSI_AGENT_PROMPT.contains("propose the missing capability"),
        "the triage must send capability gaps to rsi_propose"
    );
}

#[test]
fn the_split_between_the_two_actions_is_stated() {
    // Naming the tool is not enough; the agent has to know which situation
    // each one answers.
    assert!(
        RSI_AGENT_PROMPT.contains("Guidance vs Capability"),
        "{RSI_AGENT_PROMPT}"
    );
    assert!(
        RSI_AGENT_PROMPT.contains("self_improve"),
        "the guidance half must still be named"
    );
}

#[test]
fn proposals_are_described_as_review_not_install() {
    // A proposal that reads as an applied change would make the agent report
    // work it has not done.
    assert!(
        RSI_AGENT_PROMPT.contains("NOT installed"),
        "the prompt must be explicit that proposing is not applying"
    );
}

#[test]
fn proposing_keeps_the_repeated_pattern_bar() {
    // Without this the fix trades an empty inbox for a noisy one, which is no
    // more useful.
    assert!(
        RSI_AGENT_PROMPT.contains("REPEATED pattern"),
        "proposals must need more evidence than a single occurrence"
    );
}
