//! Tests for the one-shot plan-template retry nudge (#1103).
//!
//! The approval gate already refuses a plan whose `**Problem:**` /
//! `**Target state:**` / `**Intent:**` labels are empty. The gap this pins
//! is upstream: the write path computed those warnings and dropped them
//! into a `tracing::debug!` line, so the agent saw `success`, kept going,
//! and only the human discovered the skeleton at approval time.
//!
//! The nudge is deliberately bounded to one per plan. A non-emptiness
//! validator plus an unbounded retry teaches the model to write non-empty
//! noise that passes the check while being worth less than an empty label,
//! because nothing blocks it afterwards.

use crate::utils::plan_files::{clear_template_nudge, template_nudge, template_section_warnings};
use uuid::Uuid;

/// The scaffold `create_design_md` writes: every label present, none filled.
const EMPTY_SCAFFOLD: &str = "# T\n\n\
     ## Context\n\
     - **Problem:** \n\
     - **Target state:** \n\
     - **Intent:** \n\n\
     ## Implementation steps\n\
     1. \n   - Done when: \n";

#[test]
fn empty_scaffold_produces_a_nudge_naming_every_empty_label() {
    let sid = Uuid::new_v4();
    let warnings = template_section_warnings(EMPTY_SCAFFOLD);
    let nudge = template_nudge(sid, &warnings).expect("empty scaffold must nudge");

    for label in ["**Problem:**", "**Target state:**", "**Intent:**"] {
        assert!(
            nudge.contains(label),
            "nudge must name the empty label {label}, got: {nudge}"
        );
    }
}

#[test]
fn nudge_is_bounded_to_one_per_plan() {
    let sid = Uuid::new_v4();
    let warnings = template_section_warnings(EMPTY_SCAFFOLD);

    assert!(
        template_nudge(sid, &warnings).is_some(),
        "first incomplete write must nudge"
    );
    assert!(
        template_nudge(sid, &warnings).is_none(),
        "second must stay silent: an unbounded retry loop against a \
         non-emptiness check breeds filler that passes it"
    );
    assert!(
        template_nudge(sid, &warnings).is_none(),
        "and it must stay silent thereafter"
    );
}

#[test]
fn a_fresh_scaffold_restores_the_single_retry() {
    let sid = Uuid::new_v4();
    let warnings = template_section_warnings(EMPTY_SCAFFOLD);

    assert!(template_nudge(sid, &warnings).is_some());
    assert!(template_nudge(sid, &warnings).is_none());

    // `create_design_md` clears the mark: a new plan in the same session is
    // a new chance to get the template right.
    clear_template_nudge(sid);
    assert!(
        template_nudge(sid, &warnings).is_some(),
        "a newly scaffolded plan gets its own single retry"
    );
}

#[test]
fn a_filled_template_never_nudges() {
    let sid = Uuid::new_v4();
    let filled = "# T\n\n\
         ## Context\n\
         - **Problem:** handler.rs is 4924 lines with a 4000-line fn\n\
         - **Target state:** five extracted modules, suite green\n\
         - **Intent:** one seam per commit so a bad seam reverts alone\n\n\
         ## Implementation steps\n\
         1. extract member events\n   - Done when: suite green\n";

    let warnings = template_section_warnings(filled);
    assert!(
        warnings.is_empty(),
        "filled template has nothing to warn about"
    );
    assert!(
        template_nudge(sid, &warnings).is_none(),
        "no warnings means no nudge, and no mark spent"
    );

    // The unspent mark still works when a later write does break the template.
    let broken = template_section_warnings(EMPTY_SCAFFOLD);
    assert!(
        template_nudge(sid, &broken).is_some(),
        "a clean write must not consume the plan's single retry"
    );
}

#[test]
fn nudge_points_at_the_discussion_not_at_a_template() {
    let sid = Uuid::new_v4();
    let warnings = template_section_warnings(EMPTY_SCAFFOLD);
    let nudge = template_nudge(sid, &warnings).expect("must nudge");
    let lower = nudge.to_lowercase();

    // The labels are a transcription of decisions already made, so the fix
    // is mechanical: point at the conversation, never ship a worked example
    // (few-shot pressure toward the example's problem, not the user's) and
    // never bounce the three questions back at the user.
    assert!(
        lower.contains("conversation") || lower.contains("discussed"),
        "nudge must point at the discussion already in context: {nudge}"
    );
    assert!(
        lower.contains("filler") || lower.contains("plainly"),
        "nudge must prefer an honest gap over content that only passes the \
         non-empty check: {nudge}"
    );
}
