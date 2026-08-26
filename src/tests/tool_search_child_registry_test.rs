//! What `build_child_registry`'s re-binding costs, and what it must not cost
//! (#1210).
//!
//! The positive case — a child's `tool_search` activating into the child's
//! own registry — is covered in `tool_search_activation_test`. This file
//! covers the two things that are easy to get wrong while fixing it: the
//! mechanism being replaced, and the ownership cycle the fix invites.

use std::sync::Arc;

use uuid::Uuid;

use crate::brain::tools::registry::ToolRegistry;
use crate::brain::tools::subagent::build_child_registry;
use crate::brain::tools::tool_search::ToolSearchTool;
use crate::brain::tools::r#trait::ToolExecutionContext;

fn parent_with_tool_search() -> Arc<ToolRegistry> {
    let registry = Arc::new(ToolRegistry::new());
    crate::cli::tool_setup::register_runtime_tools(&registry, &crate::config::Config::default());
    registry.register(Arc::new(ToolSearchTool::new(&registry)));
    registry
}

#[test]
fn test_a_child_registry_is_freed_when_its_last_owner_drops() {
    // The registry OWNS its tool_search, so binding the tool back to the
    // registry with an Arc closes a registry -> tool -> registry cycle and
    // the registry is never freed. Harmless for the one long-lived parent;
    // one leaked registry per spawn otherwise. `Weak` is what makes this
    // assertion hold, and it is the only thing that does.
    let parent = parent_with_tool_search();
    let watch = {
        let child = build_child_registry(&parent);
        assert!(
            child.get("tool_search").is_some(),
            "precondition: the child carries a re-bound tool_search"
        );
        Arc::downgrade(&child)
    };
    assert!(
        watch.upgrade().is_none(),
        "#1210: the child registry outlived its owner — the re-bound \
         tool_search is holding it alive"
    );
}

#[tokio::test]
async fn test_an_unbound_search_tool_writes_to_the_wrong_registry() {
    // The mechanism the fix replaces, built by hand because
    // `build_child_registry` now re-binds and can no longer produce it. Kept
    // because it is what the report observed and what #1025's tests could not
    // see: they exercise a single registry, where the two are the same object.
    let parent = parent_with_tool_search();

    let child = Arc::new(ToolRegistry::new());
    for name in parent.list_tools() {
        if let Some(tool) = parent.get(&name) {
            child.register(tool); // copies the PARENT-bound tool_search
        }
    }

    let child_session = Uuid::new_v4();
    let query = child
        .list_tools()
        .into_iter()
        .find(|n| n != "tool_search")
        .expect("something to find");
    let search = child.get("tool_search").expect("copied from the parent");

    let result = search
        .execute(
            serde_json::json!({ "query": query }),
            &ToolExecutionContext::new(child_session),
        )
        .await
        .expect("tool_search executes");
    assert!(result.success, "{:?}", result.error);

    assert!(
        child.active_tools(child_session).is_empty(),
        "#1210: this is the bug — the child's request builder reads this, and \
         it stayed empty"
    );
    assert!(
        !parent.active_tools(child_session).is_empty(),
        "#1210: ...because the activation landed in the parent, keyed by the \
         child's session id"
    );
}
