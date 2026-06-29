use crate::brain::tools::Tool;
use crate::brain::tools::ToolExecutionContext;
use crate::brain::tools::ToolRegistry;
use crate::brain::tools::tool_manage::*;
use std::io::Write as IoWrite;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;
use tokio;
use uuid::Uuid;

fn setup() -> (Arc<ToolRegistry>, PathBuf, ToolManageTool) {
    let dir = TempDir::new().unwrap();
    let tools_path = dir.keep().join("tools.toml");
    let registry = Arc::new(ToolRegistry::new());
    let tool = ToolManageTool::new(registry.clone(), tools_path.clone());
    (registry, tools_path, tool)
}

fn ctx() -> ToolExecutionContext {
    ToolExecutionContext::new(Uuid::new_v4()).with_auto_approve(true)
}

#[tokio::test]
async fn test_list_empty() {
    let (_reg, _path, tool) = setup();
    let result = tool
        .execute(serde_json::json!({"action": "list"}), &ctx())
        .await
        .unwrap();
    assert!(result.success);
    assert!(result.output.contains("No dynamic tools"));
}

#[tokio::test]
async fn test_add_shell_tool() {
    let (reg, _path, tool) = setup();
    let result = tool
        .execute(
            serde_json::json!({
                "action": "add",
                "name": "my_echo",
                "description": "Echo a message",
                "executor": "shell",
                "command": "echo {{msg}}",
                "requires_approval": false,
                "params": [{"name": "msg", "type": "string", "required": true}]
            }),
            &ctx(),
        )
        .await
        .unwrap();
    assert!(result.success, "add failed: {:?}", result.error);
    assert!(reg.has_tool("my_echo"));
}

#[tokio::test]
async fn test_add_then_list() {
    let (_reg, _path, tool) = setup();
    tool.execute(
        serde_json::json!({
            "action": "add",
            "name": "test_tool",
            "description": "A test tool",
            "executor": "shell",
            "command": "echo test"
        }),
        &ctx(),
    )
    .await
    .unwrap();

    let result = tool
        .execute(serde_json::json!({"action": "list"}), &ctx())
        .await
        .unwrap();
    assert!(result.output.contains("test_tool"));
    assert!(result.output.contains("enabled"));
}

#[tokio::test]
async fn test_remove_tool() {
    let (reg, _path, tool) = setup();
    // Add first
    tool.execute(
        serde_json::json!({
            "action": "add",
            "name": "removable",
            "description": "Will be removed",
            "executor": "shell",
            "command": "echo bye"
        }),
        &ctx(),
    )
    .await
    .unwrap();
    assert!(reg.has_tool("removable"));

    // Remove
    let result = tool
        .execute(
            serde_json::json!({"action": "remove", "name": "removable"}),
            &ctx(),
        )
        .await
        .unwrap();
    assert!(result.success);
    assert!(!reg.has_tool("removable"));
}

#[tokio::test]
async fn test_disable_enable() {
    let (reg, _path, tool) = setup();
    tool.execute(
        serde_json::json!({
            "action": "add",
            "name": "toggleable",
            "description": "Can be toggled",
            "executor": "shell",
            "command": "echo hi"
        }),
        &ctx(),
    )
    .await
    .unwrap();
    assert!(reg.has_tool("toggleable"));

    // Disable
    let result = tool
        .execute(
            serde_json::json!({"action": "disable", "name": "toggleable"}),
            &ctx(),
        )
        .await
        .unwrap();
    assert!(result.success);
    assert!(!reg.has_tool("toggleable"));

    // Enable
    let result = tool
        .execute(
            serde_json::json!({"action": "enable", "name": "toggleable"}),
            &ctx(),
        )
        .await
        .unwrap();
    assert!(result.success);
    assert!(reg.has_tool("toggleable"));
}

#[tokio::test]
async fn test_reload() {
    let (reg, path, tool) = setup();
    // Write a tools.toml directly
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(
        f,
        r#"
[[tools]]
name = "from_disk"
description = "Loaded from disk"
executor = "shell"
command = "echo disk"
"#
    )
    .unwrap();

    let result = tool
        .execute(serde_json::json!({"action": "reload"}), &ctx())
        .await
        .unwrap();
    assert!(result.success);
    assert!(reg.has_tool("from_disk"));
}

#[tokio::test]
async fn test_add_missing_name() {
    let (_reg, _path, tool) = setup();
    let result = tool
        .execute(
            serde_json::json!({"action": "add", "executor": "shell"}),
            &ctx(),
        )
        .await
        .unwrap();
    assert!(!result.success);
}

#[tokio::test]
async fn test_add_missing_executor() {
    let (_reg, _path, tool) = setup();
    let result = tool
        .execute(
            serde_json::json!({
                "action": "add",
                "name": "no_exec",
                "description": "Missing executor"
            }),
            &ctx(),
        )
        .await
        .unwrap();
    assert!(!result.success);
}

#[tokio::test]
async fn test_unknown_action() {
    let (_reg, _path, tool) = setup();
    let result = tool
        .execute(serde_json::json!({"action": "destroy"}), &ctx())
        .await
        .unwrap();
    assert!(!result.success);
    assert!(result.error.unwrap().contains("Unknown action"));
}

#[tokio::test]
async fn test_add_http_tool() {
    let (reg, _path, tool) = setup();
    let result = tool
        .execute(
            serde_json::json!({
                "action": "add",
                "name": "health_check",
                "description": "Check server health",
                "executor": "http",
                "method": "GET",
                "url": "https://example.com/health",
                "timeout_secs": 10,
                "headers": {"Authorization": "Bearer {{token}}"},
                "params": [{"name": "token", "type": "string", "required": true}]
            }),
            &ctx(),
        )
        .await
        .unwrap();
    assert!(result.success, "add http failed: {:?}", result.error);
    assert!(reg.has_tool("health_check"));

    // Verify it shows up in tool definitions
    let defs = reg.get_tool_definitions();
    let hc = defs.iter().find(|t| t.name == "health_check").unwrap();
    assert!(hc.description.contains("health"));
}

#[tokio::test]
async fn test_remove_nonexistent() {
    let (_reg, _path, tool) = setup();
    let result = tool
        .execute(
            serde_json::json!({"action": "remove", "name": "ghost"}),
            &ctx(),
        )
        .await
        .unwrap();
    assert!(!result.success);
}
