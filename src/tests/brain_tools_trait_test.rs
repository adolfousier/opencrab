use crate::brain::tools::ToolExecutionContext;
use crate::brain::tools::ToolResult;
use uuid::Uuid;

#[test]
fn test_execution_context() {
    let session_id = Uuid::new_v4();
    let ctx = ToolExecutionContext::new(session_id)
        .with_auto_approve(true)
        .with_timeout(60);

    assert_eq!(ctx.session_id, session_id);
    assert!(ctx.auto_approve);
    assert_eq!(ctx.timeout_secs, 60);
}

#[test]
fn test_tool_result_success() {
    let result = ToolResult::success("Done!".to_string())
        .with_metadata("duration_ms".to_string(), "123".to_string());

    assert!(result.success);
    assert_eq!(result.output, "Done!");
    assert!(result.error.is_none());
    assert_eq!(result.metadata.get("duration_ms"), Some(&"123".to_string()));
}

#[test]
fn test_tool_result_error() {
    let result = ToolResult::error("Something went wrong".to_string());

    assert!(!result.success);
    assert_eq!(result.error, Some("Something went wrong".to_string()));
}
