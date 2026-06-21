//! Tests for the `plan` tool's trigger-criteria description.
//!
//! Issue: the agent was rarely calling `plan` because the tool
//! description didn't tell it WHEN to plan. A generic "Manage
//! structured task plans" left the agent to guess; it usually
//! skipped planning even on clearly multi-step work.
//!
//! These tests act as sentinels: they fail loudly if a future
//! refactor strips the trigger criteria from the description,
//! reverting plan to its pre-2026-05-30 invisibility.

use crate::brain::tools::Tool;
use crate::brain::tools::plan_tool::PlanTool;

fn tool_description() -> &'static str {
    PlanTool.description()
}

#[test]
fn description_includes_explicit_when_to_use_section() {
    let d = tool_description();
    assert!(
        d.contains("WHEN TO USE"),
        "description must call out trigger criteria explicitly so the \
         agent doesn't skip planning on multi-step work; got: {d}"
    );
}

#[test]
fn description_mentions_three_plus_distinct_steps_trigger() {
    let d = tool_description();
    assert!(
        d.contains("3+") || d.contains("three"),
        "description must specify the minimum step count that warrants \
         a plan; got: {d}"
    );
}

#[test]
fn description_mentions_dependency_trigger() {
    let d = tool_description();
    assert!(
        d.contains("dependenc"),
        "description must call out inter-step dependencies as a trigger; got: {d}"
    );
}

#[test]
fn description_mentions_multi_file_trigger() {
    let d = tool_description();
    assert!(
        d.to_lowercase().contains("multiple files") || d.to_lowercase().contains("multi-file"),
        "description must call out multi-file work as a trigger; got: {d}"
    );
}

#[test]
fn description_mentions_user_explicit_request_trigger() {
    let d = tool_description();
    assert!(
        d.to_lowercase().contains("user explicitly") || d.to_lowercase().contains("user asks"),
        "description must mention explicit user request for a plan; got: {d}"
    );
}

#[test]
fn description_calls_out_compaction_persistence() {
    // The plan persists across compactions and acts as durable
    // memory for long sessions — important context for the agent
    // when deciding whether the upfront cost of planning pays off.
    let d = tool_description();
    assert!(
        d.contains("compaction"),
        "description must mention compaction persistence so the agent \
         knows the plan doubles as long-session memory; got: {d}"
    );
}

#[test]
fn description_explicitly_carves_out_trivial_work() {
    let d = tool_description();
    assert!(
        d.contains("Skip") || d.contains("trivial"),
        "description must carve out trivial cases so the agent doesn't \
         over-plan single-tool answers; got: {d}"
    );
}

#[test]
fn description_keeps_capability_summary_and_names_the_four_commands() {
    // Keep the capability framing AND name the 4 commands the agent maps to.
    let d = tool_description();
    assert!(
        d.contains("structured task plan") || d.contains("plan-and-execute"),
        "description must keep the capability summary alongside the trigger criteria; got: {d}"
    );
    for cmd in ["init", "add_task", "start", "complete"] {
        assert!(
            d.contains(cmd),
            "description must name the `{cmd}` command so the agent can map to it; got: {d}"
        );
    }
}
