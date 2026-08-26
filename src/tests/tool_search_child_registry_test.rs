//! `tool_search` activates into the registry the child's request builder
//! reads (#1210, follow-up to #1025).
//!
//! `build_child_registry` copies the parent's `Arc<dyn Tool>` values, so a
//! `ToolSearchTool` constructed against the parent kept pointing there after
//! the copy. In a child session it activated into the PARENT registry under
//! the CHILD's session id, while the child's request builder read the child's
//! own active set. The searched tool's schema never rode on the child's next
//! request, so the model could not call what it had just been told was
//! callable — and two models on two providers converged on the same
//! byte-identical re-search loop until the loop-breaker killed the turn.
//!
//! #1025's tests passed because they exercise a single registry in isolation:
//! none mounts a child with a copied registry and an externally-bound tool.

use std::sync::Arc;

use uuid::Uuid;

use crate::brain::tools::registry::ToolRegistry;
use crate::brain::tools::subagent::build_child_registry;
use crate::brain::tools::tool_search::ToolSearchTool;
use crate::brain::tools::r#trait::ToolExecutionContext;

/// A parent registry carrying a searchable tool plus `tool_search`, wired the
/// way `register_core_agent_tools` wires it.
fn parent_with_tool_search() -> Arc<ToolRegistry> {
    let registry = Arc::new(ToolRegistry::new());
    crate::cli::tool_setup::register_runtime_tools(&registry, &crate::config::Config::default());
    registry.register(Arc::new(ToolSearchTool::new(&registry)));
    registry
}

fn ctx(session_id: Uuid) -> ToolExecutionContext {
    ToolExecutionContext::new(session_id)
}

#[tokio::test]
async fn test_child_search_activates_in_the_child_registry() {
    let parent = parent_with_tool_search();
    let child = Arc::new(build_child_registry(&parent));
    // The rebinding spawn.rs performs.
    child.register(Arc::new(ToolSearchTool::new(&child)));

    let Some(search) = child.get("tool_search") else {
        panic!("tool_search must be present in the child registry");
    };

    let child_session = Uuid::new_v4();
    let query = child
        .list_tools()
        .into_iter()
        .find(|n| n != "tool_search")
        .expect("child registry has something to find");

    let result = search
        .execute(serde_json::json!({ "query": query }), &ctx(child_session))
        .await
        .expect("tool_search executes");
    assert!(result.success, "tool_search failed: {:?}", result.error);

    assert!(
        !child.active_tools(child_session).is_empty(),
        "#1210: the child's own active set is what its request builder reads, \
         and it stayed empty — the schema never rides on the child's request"
    );
    assert!(
        parent.active_tools(child_session).is_empty(),
        "#1210: the child's activation must not land in the parent's registry"
    );
}

#[tokio::test]
async fn test_an_unrebound_child_search_writes_to_the_wrong_registry() {
    // The pre-fix arrangement, kept as documentation of the failure: a child
    // registry whose tool_search still points at the parent. This is what
    // spawn.rs must not leave behind.
    let parent = parent_with_tool_search();
    let child = Arc::new(build_child_registry(&parent));

    let search = child.get("tool_search").expect("copied from the parent");
    let child_session = Uuid::new_v4();
    let query = child
        .list_tools()
        .into_iter()
        .find(|n| n != "tool_search")
        .expect("child registry has something to find");

    let result = search
        .execute(serde_json::json!({ "query": query }), &ctx(child_session))
        .await
        .expect("tool_search executes");
    assert!(result.success);

    assert!(
        child.active_tools(child_session).is_empty(),
        "#1210: this is the bug — the copied tool wrote somewhere else"
    );
    assert!(
        !parent.active_tools(child_session).is_empty(),
        "#1210: ...namely the parent, keyed by the child's session id"
    );
}
