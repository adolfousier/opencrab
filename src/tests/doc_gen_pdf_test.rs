//! Tests for the `generate_document` PDF backend (#357).
//!
//! Generated PDFs are read back with `pdf-extract`, the same crate the
//! read side uses, so the round trip proves our own tooling can consume
//! what we write.

use crate::brain::tools::doc_gen::docx::BlockSpec;
use crate::brain::tools::doc_gen::pdf::write_pdf;
use serde_json::json;

fn blocks(v: serde_json::Value) -> Vec<BlockSpec> {
    serde_json::from_value(v).expect("valid block specs")
}

#[test]
fn pdf_contains_heading_paragraph_list_and_table_text() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("report.pdf");
    let summary = write_pdf(
        &path,
        &blocks(json!([
            {"type": "heading", "text": "Annual Summary", "level": 1},
            {"type": "paragraph", "text": "All systems operated normally."},
            {"type": "list", "items": ["uptime held", "costs flat"], "ordered": true},
            {"type": "table", "rows": [["Metric", "Value"], ["Requests", 12345]]}
        ])),
        "Annual Summary",
    )
    .expect("pdf written");
    assert!(summary.contains("1 page(s)"));

    let text = pdf_extract::extract_text(&path).expect("pdf extracts");
    assert!(text.contains("Annual Summary"), "text: {text}");
    assert!(text.contains("All systems operated normally."));
    assert!(text.contains("uptime held"));
    assert!(text.contains("1."));
    assert!(text.contains("Requests"));
    assert!(text.contains("12345"));
}

#[test]
fn long_content_breaks_across_pages() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("long.pdf");
    let many: Vec<serde_json::Value> = (0..120)
        .map(|i| json!({"type": "paragraph", "text": format!("Paragraph number {i} of filler content.")}))
        .collect();
    let summary = write_pdf(&path, &blocks(json!(many)), "Long").expect("pdf written");
    let pages: usize = summary
        .split_whitespace()
        .next()
        .and_then(|n| n.parse().ok())
        .expect("summary starts with page count");
    assert!(
        pages >= 2,
        "expected multiple pages, got summary: {summary}"
    );

    let text = pdf_extract::extract_text(&path).expect("pdf extracts");
    assert!(text.contains("Paragraph number 0"));
    assert!(text.contains("Paragraph number 119"));
}

#[test]
fn long_paragraph_wraps_instead_of_overflowing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("wrap.pdf");
    let long_text = "word ".repeat(200);
    let summary = write_pdf(
        &path,
        &blocks(json!([{"type": "paragraph", "text": long_text}])),
        "Wrap",
    )
    .expect("pdf written");
    // 200 short words cannot fit one line: the layout must emit many lines.
    let lines: usize = summary
        .split("page(s), ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .expect("summary includes line count");
    assert!(lines > 5, "expected wrapped lines, got summary: {summary}");
}
