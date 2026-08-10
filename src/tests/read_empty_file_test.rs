//! An empty file must announce itself. Silence from `read_file` is
//! indistinguishable from a failed read or a wrong path, and the model burns
//! turns re-reading and guessing path variants (#987, from the Command Code
//! read_file audit: silence is the most expensive thing a tool can return).

use crate::brain::tools::read::ReadTool;
use crate::brain::tools::{Tool, ToolExecutionContext};
use uuid::Uuid;

async fn read(path: &str) -> crate::brain::tools::ToolResult {
    let tool = ReadTool;
    let ctx = ToolExecutionContext::new(Uuid::new_v4());
    tool.execute(serde_json::json!({ "path": path }), &ctx)
        .await
        .unwrap()
}

#[tokio::test]
async fn empty_file_returns_explicit_note_not_silence() {
    let f = tempfile::Builder::new().suffix(".txt").tempfile().unwrap();

    let result = read(f.path().to_str().unwrap()).await;
    assert!(result.success, "reading an empty file must succeed");
    assert_eq!(result.output, "(file exists and is empty, 0 bytes)");
}

#[tokio::test]
async fn empty_file_note_also_covers_ranged_reads() {
    let f = tempfile::Builder::new().suffix(".txt").tempfile().unwrap();

    let tool = ReadTool;
    let ctx = ToolExecutionContext::new(Uuid::new_v4());
    let result = tool
        .execute(
            serde_json::json!({
                "path": f.path().to_str().unwrap(),
                "start_line": 0,
                "line_count": 10
            }),
            &ctx,
        )
        .await
        .unwrap();

    assert!(result.success, "ranged read of an empty file must succeed");
    assert_eq!(result.output, "(file exists and is empty, 0 bytes)");
}
