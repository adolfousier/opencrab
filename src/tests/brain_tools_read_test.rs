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

// ── #1164: non-regular files rejected fast and self-describing ───────

/// The issue's headline scenario: a char-device path must come back as an
/// error well inside the tool timeout — never hang the call. The gate has
/// existed since v0.2.21; this pins it plus the self-describing kind.
#[cfg(unix)]
#[tokio::test]
async fn test_read_char_device_rejected_fast() {
    let tool = ReadTool;
    let context = ToolExecutionContext::new(Uuid::new_v4());

    let input = serde_json::json!({ "path": "/dev/zero" });

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tool.execute(input, &context),
    )
    .await
    .expect("read_file(/dev/zero) hung instead of erroring")
    .unwrap();
    assert!(!result.success);
    let err = result.error.as_deref().unwrap_or_default();
    assert!(
        err.contains("character device"),
        "error should name the file type: {err}"
    );
}

#[tokio::test]
async fn test_read_directory_names_the_kind() {
    let temp_dir = TempDir::new().unwrap();
    let tool = ReadTool;
    let context = ToolExecutionContext::new(Uuid::new_v4())
        .with_working_directory(temp_dir.path().to_path_buf());

    let result = tool
        .execute(serde_json::json!({ "path": "." }), &context)
        .await
        .unwrap();
    assert!(!result.success);
    let err = result.error.as_deref().unwrap_or_default();
    assert!(
        err.contains("directory"),
        "error should name the file type: {err}"
    );
}
