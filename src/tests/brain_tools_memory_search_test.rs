use super::*;
use crate::brain::tools::ToolExecutionContext;
use tokio;

#[test]
fn test_tool_metadata() {
    let tool = MemorySearchTool;
    assert_eq!(tool.name(), "memory_search");
    assert!(!tool.requires_approval());
}

#[tokio::test]
async fn test_empty_query() {
    let tool = MemorySearchTool;
    let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    let result = tool
        .execute(serde_json::json!({"query": ""}), &ctx)
        .await
        .unwrap();
    assert!(!result.success);
}
