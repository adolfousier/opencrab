use crate::brain::tools::Tool;
use crate::brain::tools::ToolExecutionContext;
use crate::brain::tools::config_tool::*;
use tokio;

#[test]
fn test_tool_metadata() {
    let tool = ConfigTool;
    assert_eq!(tool.name(), "config_manager");
    assert!(tool.requires_approval());
}

#[tokio::test]
async fn test_unknown_operation() {
    let tool = ConfigTool;
    let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    let result = tool
        .execute(serde_json::json!({"operation": "nope"}), &ctx)
        .await
        .unwrap();
    assert!(!result.success);
    assert!(result.error.unwrap().contains("Unknown operation"));
}

#[tokio::test]
async fn test_read_config() {
    let tool = ConfigTool;
    let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    let result = tool
        .execute(
            serde_json::json!({"operation": "read_config", "section": "agent"}),
            &ctx,
        )
        .await
        .unwrap();
    // Should succeed even with default config
    assert!(result.success);
    assert!(result.output.contains("approval_policy"));
}

#[tokio::test]
async fn test_read_commands_empty() {
    let tool = ConfigTool;
    let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    let result = tool
        .execute(serde_json::json!({"operation": "read_commands"}), &ctx)
        .await
        .unwrap();
    assert!(result.success);
}

#[tokio::test]
async fn test_write_config_missing_fields() {
    let tool = ConfigTool;
    let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    let result = tool
        .execute(serde_json::json!({"operation": "write_config"}), &ctx)
        .await
        .unwrap();
    assert!(!result.success);
}
