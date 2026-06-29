use crate::brain::tools::ToolError;
use crate::brain::tools::ToolExecutionContext;
use crate::brain::tools::ToolRegistry;
use crate::brain::tools::ToolResult;
use crate::brain::tools::r#trait::Tool;
use crate::brain::tools::{Result, ToolCapability};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tokio;
use uuid::Uuid;

/// Mock tool for testing
struct MockTool {
    name: String,
    requires_approval: bool,
}

#[async_trait]
impl Tool for MockTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "A mock tool for testing"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "Test message"
                }
            },
            "required": ["message"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadFiles]
    }

    fn requires_approval(&self) -> bool {
        self.requires_approval
    }

    async fn execute(&self, _input: Value, _context: &ToolExecutionContext) -> Result<ToolResult> {
        Ok(ToolResult::success("Mock execution successful".to_string()))
    }
}

#[test]
fn test_registry_creation() {
    let registry = ToolRegistry::new();
    assert_eq!(registry.count(), 0);
}

#[test]
fn test_register_tool() {
    let registry = ToolRegistry::new();
    let tool = Arc::new(MockTool {
        name: "test_tool".to_string(),
        requires_approval: false,
    });

    registry.register(tool);
    assert_eq!(registry.count(), 1);
    assert!(registry.has_tool("test_tool"));
    assert!(!registry.has_tool("nonexistent"));
}

#[test]
fn test_list_tools() {
    let registry = ToolRegistry::new();

    registry.register(Arc::new(MockTool {
        name: "tool1".to_string(),
        requires_approval: false,
    }));
    registry.register(Arc::new(MockTool {
        name: "tool2".to_string(),
        requires_approval: false,
    }));

    let tools = registry.list_tools();
    assert_eq!(tools.len(), 2);
    assert!(tools.contains(&"tool1".to_string()));
    assert!(tools.contains(&"tool2".to_string()));
}

#[tokio::test]
async fn test_execute_tool() {
    let registry = ToolRegistry::new();
    let tool = Arc::new(MockTool {
        name: "test_tool".to_string(),
        requires_approval: false,
    });

    registry.register(tool);

    let session_id = Uuid::new_v4();
    let context = ToolExecutionContext::new(session_id);
    let input = serde_json::json!({ "message": "test" });

    let result = registry
        .execute("test_tool", input, &context)
        .await
        .unwrap();
    assert!(result.success);
    assert_eq!(result.output, "Mock execution successful");
}

#[tokio::test]
async fn test_execute_nonexistent_tool() {
    let registry = ToolRegistry::new();
    let session_id = Uuid::new_v4();
    let context = ToolExecutionContext::new(session_id);
    let input = serde_json::json!({});

    let result = registry.execute("nonexistent", input, &context).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ToolError::NotFound(_)));
}

#[tokio::test]
async fn test_execute_requires_approval() {
    let registry = ToolRegistry::new();
    let tool = Arc::new(MockTool {
        name: "dangerous_tool".to_string(),
        requires_approval: true,
    });

    registry.register(tool);

    let session_id = Uuid::new_v4();
    let context = ToolExecutionContext::new(session_id); // auto_approve = false
    let input = serde_json::json!({ "message": "test" });

    let result = registry.execute("dangerous_tool", input, &context).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        ToolError::ApprovalRequired(_)
    ));
}

/// Mock tool whose input validation always fails — used to prove JIT
/// activation (#214) runs BEFORE validation, so a tool that fails its
/// first (blind) call is still discovered for the next turn.
struct ValidateFailTool;

#[async_trait]
impl Tool for ValidateFailTool {
    fn name(&self) -> &str {
        "extended_blind_tool"
    }
    fn description(&self) -> &str {
        "always fails validation"
    }
    fn input_schema(&self) -> Value {
        serde_json::json!({ "type": "object" })
    }
    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadFiles]
    }
    fn validate_input(&self, _input: &Value) -> Result<()> {
        Err(ToolError::InvalidInput("missing required param".into()))
    }
    async fn execute(&self, _input: Value, _context: &ToolExecutionContext) -> Result<ToolResult> {
        Ok(ToolResult::success("unreachable".to_string()))
    }
}

#[tokio::test]
async fn test_execute_jit_activates_extended_tool_even_on_failure() {
    // #214: an EXTENDED tool called by name but never surfaced via
    // tool_search must be activated for the session so its schema rides the
    // NEXT request, and that activation has to happen even when this first
    // call fails on bad params, or the model stays stuck guessing blind.
    let registry = ToolRegistry::new();
    registry.register(Arc::new(ValidateFailTool));

    let session_id = Uuid::new_v4();
    let context = ToolExecutionContext::new(session_id);

    assert!(
        !registry
            .active_tools(session_id)
            .contains("extended_blind_tool")
    );

    let result = registry
        .execute("extended_blind_tool", serde_json::json!({}), &context)
        .await;

    // The call failed validation...
    assert!(matches!(result.unwrap_err(), ToolError::InvalidInput(_)));
    // ...yet the tool is now discovered for the next turn.
    assert!(
        registry
            .active_tools(session_id)
            .contains("extended_blind_tool"),
        "extended tool must be activated before validation, even on a failing call"
    );
}

#[tokio::test]
async fn test_execute_does_not_activate_core_tool() {
    // CORE tools are always in the prompt, so JIT activation must skip them
    // (no point bloating the per-session active set). A MockTool registered
    // under a core name (`bash`) must NOT be added to the active set.
    let registry = ToolRegistry::new();
    registry.register(Arc::new(MockTool {
        name: "bash".to_string(),
        requires_approval: false,
    }));

    let session_id = Uuid::new_v4();
    let context = ToolExecutionContext::new(session_id);

    let result = registry
        .execute("bash", serde_json::json!({ "message": "hi" }), &context)
        .await
        .unwrap();
    assert!(result.success);
    assert!(
        !registry.active_tools(session_id).contains("bash"),
        "core tools must never be added to the session active set"
    );
}

#[tokio::test]
async fn test_execute_with_auto_approve() {
    let registry = ToolRegistry::new();
    let tool = Arc::new(MockTool {
        name: "dangerous_tool".to_string(),
        requires_approval: true,
    });

    registry.register(tool);

    let session_id = Uuid::new_v4();
    let context = ToolExecutionContext::new(session_id).with_auto_approve(true);
    let input = serde_json::json!({ "message": "test" });

    let result = registry
        .execute("dangerous_tool", input, &context)
        .await
        .unwrap();
    assert!(result.success);
}
