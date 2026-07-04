//! Tests for the `goal_manage` self-goaling tool (#307).
//!
//! The tool is a pure wrapper over `GoalManager`, so these lock the surface the
//! model sees (name, description, schema) and the guard behavior that does not
//! need a live DB — the CRUD itself is covered by the goal system's own tests.

use crate::brain::tools::goal_manage::GoalManageTool;
use crate::brain::tools::r#trait::{Tool, ToolCapability, ToolExecutionContext};

#[test]
fn name_is_goal_manage() {
    assert_eq!(GoalManageTool.name(), "goal_manage");
}

#[test]
fn description_conveys_self_goaling() {
    let d = GoalManageTool.description().to_lowercase();
    assert!(d.contains("goal"));
    // It must read as autonomous multi-turn driving, not just a getter.
    assert!(d.contains("autonomous") || d.contains("autonomously"));
    assert!(d.contains("turn"));
}

#[test]
fn schema_exposes_the_five_actions_and_requires_action() {
    let schema = GoalManageTool.input_schema();
    let actions = schema["properties"]["action"]["enum"]
        .as_array()
        .expect("action enum present");
    let actions: Vec<&str> = actions.iter().filter_map(|v| v.as_str()).collect();
    for a in ["set", "status", "pause", "resume", "clear"] {
        assert!(actions.contains(&a), "action '{a}' missing from schema");
    }
    let required = schema["required"].as_array().expect("required present");
    assert!(required.iter().any(|v| v.as_str() == Some("action")));
    // 'goal' is the payload for `set`.
    assert!(schema["properties"]["goal"].is_object());
}

#[test]
fn managing_a_goal_never_requires_approval() {
    // Session-local, bounded state — no approval gate on any action.
    let input = serde_json::json!({"action": "set", "goal": "all tests green"});
    assert!(!GoalManageTool.requires_approval_for_input(&input));
    let input = serde_json::json!({"action": "clear"});
    assert!(!GoalManageTool.requires_approval_for_input(&input));
}

#[test]
fn declares_system_modification_capability() {
    assert!(
        GoalManageTool
            .capabilities()
            .contains(&ToolCapability::SystemModification)
    );
}

#[tokio::test]
async fn errors_cleanly_without_a_service_context() {
    // No service_context (e.g. A2A) → a clear error, never a panic.
    let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    let input = serde_json::json!({"action": "status"});
    let result = GoalManageTool.execute(input, &ctx).await.unwrap();
    assert!(!result.success);
    assert!(result.error.unwrap_or_default().contains("Service context"));
}
