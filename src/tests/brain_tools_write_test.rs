use crate::brain::tools::Tool;
use crate::brain::tools::ToolCapability;
use crate::brain::tools::ToolExecutionContext;
use crate::brain::tools::write::*;
use tempfile::TempDir;
use tokio;
use uuid::Uuid;

#[tokio::test]
async fn test_write_file() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.txt");

    let tool = WriteTool;
    let session_id = Uuid::new_v4();
    let context =
        ToolExecutionContext::new(session_id).with_working_directory(temp_dir.path().to_path_buf());

    let input = serde_json::json!({
        "path": "test.txt",
        "content": "Hello, World!"
    });

    let result = tool.execute(input, &context).await.unwrap();
    assert!(result.success);

    // Verify file was written
    let contents = tokio::fs::read_to_string(&file_path).await.unwrap();
    assert_eq!(contents, "Hello, World!");
}

#[tokio::test]
async fn test_write_file_with_create_dirs() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("subdir").join("test.txt");

    let tool = WriteTool;
    let session_id = Uuid::new_v4();
    let context =
        ToolExecutionContext::new(session_id).with_working_directory(temp_dir.path().to_path_buf());

    let input = serde_json::json!({
        "path": "subdir/test.txt",
        "content": "Nested file",
        "create_dirs": true
    });

    let result = tool.execute(input, &context).await.unwrap();
    assert!(result.success);

    // Verify file was written
    let contents = tokio::fs::read_to_string(&file_path).await.unwrap();
    assert_eq!(contents, "Nested file");
}

#[tokio::test]
async fn test_write_file_missing_parent_dir() {
    let temp_dir = TempDir::new().unwrap();

    let tool = WriteTool;
    let session_id = Uuid::new_v4();
    let context =
        ToolExecutionContext::new(session_id).with_working_directory(temp_dir.path().to_path_buf());

    let input = serde_json::json!({
        "path": "nonexistent/test.txt",
        "content": "Should fail",
        "create_dirs": false
    });

    let result = tool.execute(input, &context).await.unwrap();
    assert!(!result.success);
    assert!(result.error.is_some());
}

#[test]
fn test_write_tool_schema() {
    let tool = WriteTool;
    assert_eq!(tool.name(), "write_file");
    assert!(tool.requires_approval());

    let capabilities = tool.capabilities();
    assert!(capabilities.contains(&ToolCapability::WriteFiles));
    assert!(capabilities.contains(&ToolCapability::SystemModification));
}

#[tokio::test]
async fn test_overwrite_existing_file() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.txt");

    // Write initial content
    tokio::fs::write(&file_path, "Initial content")
        .await
        .unwrap();

    let tool = WriteTool;
    let session_id = Uuid::new_v4();
    let context =
        ToolExecutionContext::new(session_id).with_working_directory(temp_dir.path().to_path_buf());

    let input = serde_json::json!({
        "path": "test.txt",
        "content": "New content",
        "overwrite_read_confirm": true
    });

    let result = tool.execute(input, &context).await.unwrap();
    assert!(result.success);

    // Verify file was overwritten
    let contents = tokio::fs::read_to_string(&file_path).await.unwrap();
    assert_eq!(contents, "New content");
}
