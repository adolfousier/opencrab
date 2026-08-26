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

use crate::brain::tools::catalog;
use crate::brain::tools::registry::ToolRegistry;
use crate::brain::tools::subagent;
use crate::brain::tools::tool_search::ToolSearchTool;
use crate::brain::tools::{self, Tool, ToolExecutionContext};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
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

/// Minimal EXTENDED tool for discovery tests — deliberately outside
/// CORE_TOOLS so `search_tools` matches it (it filters out core tools) and
/// activation is what puts it on the request.
struct ProbeTool;

#[async_trait]
impl Tool for ProbeTool {
    fn name(&self) -> &str {
        "probe_channel_send"
    }
    fn description(&self) -> &str {
        "Send a probe message on a channel; #1210 discovery regression fixture."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {}})
    }
    fn capabilities(&self) -> Vec<tools::ToolCapability> {
        vec![]
    }
    async fn execute(
        &self,
        _input: Value,
        _ctx: &ToolExecutionContext,
    ) -> tools::error::Result<tools::ToolResult> {
        Ok(tools::ToolResult::success("sent".to_string()))
    }
}

/// Regression for #1210: a child's tool_search must activate into the CHILD's
/// registry, not the parent's.
///
/// Production wiring binds the discovery tool to the PARENT registry at
/// startup (tool_setup), then `build_child_registry` copies those instances
/// into a fresh child registry. Pre-fix, the child inherited the parent-bound
/// instance, so a search executed under the child's session id wrote the
/// parent's active-set while the child's request builder read its OWN
/// (empty) one — the discovered schema never rode on a child request, and
/// well-behaved models looped re-searching instead of emitting calls for
/// undeclared tools. Observed live on two models across two providers.
#[tokio::test]
async fn child_tool_search_activates_into_the_child_registry() {
    // Parent registry with the discovery tool bound to ITSELF, exactly as
    // startup wiring does.
    let parent = Arc::new(ToolRegistry::new());
    parent.register(Arc::new(ProbeTool));
    parent.register(Arc::new(ToolSearchTool::new(&parent)));

    // Child registry built the way spawn/resume/team_create build theirs.
    let child = subagent::build_child_registry(&parent);

    // The child must carry its OWN discovery instance...
    let child_search = child
        .get(catalog::TOOL_SEARCH_NAME)
        .expect("child must inherit tool_search");

    let session = Uuid::new_v4();
    let ctx = ToolExecutionContext::new(session);
    let result = child_search
        .execute(json!({"query": "send probe message on channel"}), &ctx)
        .await
        .expect("child tool_search executes");
    assert!(result.success, "search failed: {:?}", result.error);

    // THE assertion (#1210): the discovered tool's schema now rides on the
    // CHILD's next request.
    assert!(
        child.active_tools(session).contains("probe_channel_send"),
        "#1210: child's tool_search must activate into the CHILD registry; \
         pre-fix the write landed in the parent's registry while the child \
         read its own empty active set, so the schema never rode"
    );

    // ...and a child's search must never mutate the PARENT's active set.
    assert!(
        !parent.active_tools(session).contains("probe_channel_send"),
        "a child's tool_search writes its own registry, never the parent's"
    );
}
