use crate::brain::tools::Tool;
use crate::brain::tools::ToolExecutionContext;
use crate::brain::tools::read::*;
use std::io::Write;
use tempfile::TempDir;
use tokio;
use uuid::Uuid;

#[tokio::test]
async fn test_read_file() {
    let temp_dir = TempDir::new().unwrap();
    let temp_file_path = temp_dir.path().join("test.txt");
    let mut temp_file = std::fs::File::create(&temp_file_path).unwrap();
    writeln!(temp_file, "Line 1\nLine 2\nLine 3").unwrap();
    temp_file.flush().unwrap();

    let tool = ReadTool;
    let session_id = Uuid::new_v4();
    let context =
        ToolExecutionContext::new(session_id).with_working_directory(temp_dir.path().to_path_buf());

    let input = serde_json::json!({
        "path": temp_file_path.to_str().unwrap()
    });

    let result = tool.execute(input, &context).await.unwrap();
    assert!(result.success);
    assert!(result.output.contains("Line 1"));
    assert!(result.output.contains("Line 3"));
}

#[tokio::test]
async fn test_read_file_line_range() {
    let temp_dir = TempDir::new().unwrap();
    let temp_file_path = temp_dir.path().join("test.txt");
    let mut temp_file = std::fs::File::create(&temp_file_path).unwrap();
    writeln!(temp_file, "Line 1\nLine 2\nLine 3\nLine 4\nLine 5").unwrap();
    temp_file.flush().unwrap();

    let tool = ReadTool;
    let session_id = Uuid::new_v4();
    let context =
        ToolExecutionContext::new(session_id).with_working_directory(temp_dir.path().to_path_buf());

    let input = serde_json::json!({
        "path": temp_file_path.to_str().unwrap(),
        "start_line": 1,
        "line_count": 2
    });

    let result = tool.execute(input, &context).await.unwrap();
    assert!(result.success);
    assert!(result.output.contains("Line 2"));
    assert!(result.output.contains("Line 3"));
    assert!(!result.output.contains("Line 1"));
    assert!(!result.output.contains("Line 4"));
}

#[tokio::test]
async fn test_read_nonexistent_file() {
    let temp_dir = TempDir::new().unwrap();
    let tool = ReadTool;
    let session_id = Uuid::new_v4();
    let context =
        ToolExecutionContext::new(session_id).with_working_directory(temp_dir.path().to_path_buf());

    let input = serde_json::json!({
        "path": "nonexistent_file.txt"
    });

    let result = tool.execute(input, &context).await.unwrap();
    assert!(!result.success);
    assert!(result.error.is_some());
    assert!(result.error.unwrap().contains("not found"));
}

#[test]
fn test_read_tool_schema() {
    let tool = ReadTool;
    assert_eq!(tool.name(), "read_file");
    assert!(!tool.requires_approval());

    let schema = tool.input_schema();
    assert!(schema.is_object());
}
