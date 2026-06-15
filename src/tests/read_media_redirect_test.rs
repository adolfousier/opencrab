//! `read_file` must bounce binary media to the right tool instead of dumping
//! garbage bytes — otherwise the agent loops on read_file for a dropped
//! screenshot rather than calling analyze_image.

use crate::brain::tools::read::ReadTool;
use crate::brain::tools::{Tool, ToolExecutionContext};
use std::io::Write;
use uuid::Uuid;

async fn read(path: &str) -> crate::brain::tools::ToolResult {
    let tool = ReadTool;
    let ctx = ToolExecutionContext::new(Uuid::new_v4());
    tool.execute(serde_json::json!({ "path": path }), &ctx)
        .await
        .unwrap()
}

#[tokio::test]
async fn png_redirects_to_analyze_image() {
    let mut f = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
    f.write_all(b"\x89PNG\r\n\x1a\n").unwrap();
    f.flush().unwrap();

    let result = read(f.path().to_str().unwrap()).await;
    assert!(!result.success, "reading a png should not succeed as text");
    let err = result.error.unwrap();
    assert!(err.contains("analyze_image"), "should redirect: {err}");
}

#[tokio::test]
async fn pdf_redirects_to_parse_document() {
    let mut f = tempfile::Builder::new().suffix(".pdf").tempfile().unwrap();
    f.write_all(b"%PDF-1.4\n").unwrap();
    f.flush().unwrap();

    let result = read(f.path().to_str().unwrap()).await;
    assert!(!result.success);
    assert!(result.error.unwrap().contains("parse_document"));
}

#[tokio::test]
async fn text_file_still_reads_normally() {
    let mut f = tempfile::Builder::new().suffix(".txt").tempfile().unwrap();
    writeln!(f, "hello world").unwrap();
    f.flush().unwrap();

    let result = read(f.path().to_str().unwrap()).await;
    assert!(result.success, "a .txt must still read as text");
    assert!(result.output.contains("hello world"));
}
