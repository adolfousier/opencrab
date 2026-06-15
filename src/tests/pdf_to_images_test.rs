//! Tests for the `pdf_to_images` tool — the on-demand PDF page renderer
//! that lets the agent view figures/screenshots a text-only parse misses.
//!
//! Rendering itself needs a real PDF and a renderer on PATH (covered by
//! `pdf_vision_test`), so these focus on the deterministic contract:
//! schema, the `.pdf`-only guard, and the not-found path.

use crate::brain::tools::pdf_to_images::PdfToImagesTool;
use crate::brain::tools::{Tool, ToolExecutionContext};
use std::io::Write;
use uuid::Uuid;

#[test]
fn schema_requires_path_and_advertises_ranges() {
    let tool = PdfToImagesTool;
    assert_eq!(tool.name(), "pdf_to_images");
    assert!(!tool.requires_approval());

    let schema = tool.input_schema();
    assert!(schema["properties"]["path"].is_object());
    assert!(schema["properties"]["page_range"].is_object());
    let required = schema["required"].as_array().unwrap();
    assert!(required.iter().any(|v| v.as_str() == Some("path")));
}

#[tokio::test]
async fn non_pdf_path_is_rejected() {
    let mut f = tempfile::NamedTempFile::with_suffix(".txt").unwrap();
    writeln!(f, "not a pdf").unwrap();
    f.flush().unwrap();

    let tool = PdfToImagesTool;
    let context = ToolExecutionContext::new(Uuid::new_v4());
    let input = serde_json::json!({ "path": f.path().to_str().unwrap() });

    let result = tool.execute(input, &context).await.unwrap();
    assert!(!result.success);
    assert!(
        result.error.unwrap().contains("parse_document"),
        "non-pdf should point the agent at parse_document"
    );
}

#[tokio::test]
async fn nonexistent_file_errors() {
    let tool = PdfToImagesTool;
    let context = ToolExecutionContext::new(Uuid::new_v4());
    let input = serde_json::json!({ "path": "/nonexistent/whatever.pdf" });

    let result = tool.execute(input, &context).await.unwrap();
    assert!(!result.success);
    assert!(result.error.unwrap().contains("not found"));
}

#[test]
fn validate_input_rejects_garbage_pages() {
    let tool = PdfToImagesTool;
    // `pages` must be an array of integers — a string is invalid.
    let bad = serde_json::json!({ "path": "x.pdf", "pages": "1,2,3" });
    assert!(tool.validate_input(&bad).is_err());
}
