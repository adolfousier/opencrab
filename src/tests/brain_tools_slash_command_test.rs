use crate::brain::tools::Tool;
use crate::brain::tools::ToolExecutionContext;
use crate::brain::tools::slash_command::*;
use tokio;

#[test]
fn test_tool_metadata() {
    let tool = SlashCommandTool;
    assert_eq!(tool.name(), "slash_command");
    assert!(tool.requires_approval());
}

#[tokio::test]
async fn test_missing_slash() {
    // Since #1167 a missing leading '/' is autocorrected, so "cd" must
    // behave exactly like "/cd" instead of being rejected.
    let tool = SlashCommandTool;
    let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    let bare = tool
        .execute(serde_json::json!({"command": "cd"}), &ctx)
        .await
        .unwrap();
    let slashed = tool
        .execute(serde_json::json!({"command": "/cd"}), &ctx)
        .await
        .unwrap();
    assert_eq!(
        bare.success, slashed.success,
        "autocorrect makes 'cd' equivalent to '/cd'"
    );
    assert_eq!(bare.output, slashed.output);
}

#[tokio::test]
async fn test_models_returns_provider_info() {
    let tool = SlashCommandTool;
    let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    let result = tool
        .execute(serde_json::json!({"command": "/models"}), &ctx)
        .await
        .unwrap();
    assert!(result.success);
    assert!(result.output.contains("Providers"));
}

#[tokio::test]
async fn test_help_returns_commands() {
    let tool = SlashCommandTool;
    let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    let result = tool
        .execute(serde_json::json!({"command": "/help"}), &ctx)
        .await
        .unwrap();
    assert!(result.success);
    assert!(result.output.contains("/models"));
    assert!(result.output.contains("/usage"));
}

#[tokio::test]
async fn test_cd_no_args() {
    let tool = SlashCommandTool;
    let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    let result = tool
        .execute(serde_json::json!({"command": "/cd"}), &ctx)
        .await
        .unwrap();
    assert!(!result.success);
    assert!(result.error.unwrap().contains("No directory"));
}

#[tokio::test]
async fn test_unknown_command() {
    let tool = SlashCommandTool;
    let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    let result = tool
        .execute(serde_json::json!({"command": "/nonexistent"}), &ctx)
        .await
        .unwrap();
    assert!(!result.success);
    assert!(result.error.unwrap().contains("Unknown command"));
}

#[tokio::test]
async fn test_plan_lifecycle_commands_return_guidance_not_error() {
    // /plan, /execute, /discard, /show-plan are real channel/plan commands the
    // model sees in use; they must not hit the "Unknown command" error arm.
    // Instead they return actionable guidance as a successful result (#574).
    let tool = SlashCommandTool;
    let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    for cmd in ["/plan", "/execute", "/discard", "/show-plan"] {
        let result = tool
            .execute(serde_json::json!({ "command": cmd }), &ctx)
            .await
            .unwrap();
        assert!(
            result.success,
            "{cmd} should return guidance as success, not an Unknown-command error"
        );
        assert!(
            result.output.to_lowercase().contains("plan"),
            "{cmd} guidance should mention the plan: {}",
            result.output
        );
    }
}

#[tokio::test]
async fn test_new_and_cowork_return_guidance_not_error() {
    let tool = SlashCommandTool;
    let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    for cmd in ["/new", "/clear", "/cowork"] {
        let result = tool
            .execute(serde_json::json!({ "command": cmd }), &ctx)
            .await
            .unwrap();
        assert!(
            result.success,
            "{cmd} should return guidance as success, not an error"
        );
    }
}
