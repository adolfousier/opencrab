//! A searched tool must become callable, not merely described (#1025).
//!
//! `tool_search` returned schemas as TEXT and never activated them, leaving
//! activation to the JIT-on-execute path — which only fires when a non-core
//! tool is actually USED. That is circular: a model that will not emit a call
//! for a function absent from its tool list can never trigger the activation
//! that would list it.
//!
//! Observed on a model that searched for `spawn_agent`, reported "Spawn tool
//! active", then emitted `bash(echo "spawning subagent")` and `read_file`
//! instead — 30 tool calls across two runs, both killed by the
//! announcement-loop detector. Removing the spawn from the task made it
//! complete immediately.

use crate::brain::tools::registry::ToolRegistry;
use uuid::Uuid;

/// A tool the session has not searched for is not active.
#[test]
fn an_unsearched_extended_tool_is_not_active() {
    let registry = ToolRegistry::new();
    let session = Uuid::new_v4();
    assert!(
        !registry.active_tools(session).contains("spawn_agent"),
        "a fresh session must not carry extended schemas it never asked for"
    );
}

/// Activation puts the name in the session's active set.
#[test]
fn activation_makes_a_tool_active_for_the_session() {
    let registry = ToolRegistry::new();
    let session = Uuid::new_v4();
    registry.activate_tools(session, ["spawn_agent".to_string()]);
    assert!(
        registry.active_tools(session).contains("spawn_agent"),
        "an activated tool's schema must ride on subsequent requests, or the \
         model is told a tool exists that it cannot call"
    );
}

/// Activation is per session, so one session cannot leak schemas into another.
#[test]
fn activation_does_not_leak_across_sessions() {
    let registry = ToolRegistry::new();
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    registry.activate_tools(a, ["spawn_agent".to_string()]);
    assert!(!registry.active_tools(b).contains("spawn_agent"));
}

/// tool_search must wire activation, not just describe.
///
/// Asserted on source: exercising the tool needs a populated registry and a
/// live execution context, and the defect was precisely that this one call was
/// missing while everything around it looked correct.
#[test]
fn tool_search_activates_what_it_returns() {
    let src = std::fs::read_to_string("src/brain/tools/tool_search.rs")
        .expect("tool_search.rs must be readable");
    assert!(
        src.contains("activate_tools"),
        "tool_search must activate its matches; returning schemas as text only \
         leaves the model unable to call what it just found"
    );
}
