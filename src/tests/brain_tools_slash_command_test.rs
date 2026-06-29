use super::*;
use crate::brain::tools::ToolExecutionContext;
use tokio;

#[test]
fn test_tool_metadata() {
    let tool = SlashCommandTool;
    assert_eq!(tool.name(), "slash_command");
    assert!(tool.requires_approval());
}

#[tokio::test]
async fn test_missing_slash() {
    let tool = SlashCommandTool;
    let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    let result = tool
        .execute(serde_json::json!({"command": "cd"}), &ctx)
        .await
        .unwrap();
    assert!(!result.success);
    assert!(result.error.unwrap().contains("must start with '/'"));
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
