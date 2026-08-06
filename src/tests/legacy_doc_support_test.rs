//! Tests for legacy document support in `parse_document` (#955).
//!
//! Covers the two gaps closed by the issue:
//! - Spreadsheet formats calamine already reads but the dispatch never
//!   routed to it: XLSM, XLSB, ODS.
//! - Legacy Word 97-2003 binary `.doc` (OLE/CFB) text extraction via `rwml`.

use crate::brain::tools::Tool;
use crate::brain::tools::ToolExecutionContext;
use crate::brain::tools::doc_parser::DocParserTool;
use std::io::Write;
use tempfile::NamedTempFile;
use uuid::Uuid;

/// Genuine Word 97 binary from rwml's public MIT-licensed test corpus
/// (`floating_text_bearing.doc`), committed as a fixture.
const LEGACY_DOC_BYTES: &[u8] = include_bytes!("fixtures/legacy_word_floating_text.doc");

fn parse_input(path: &std::path::Path) -> serde_json::Value {
    serde_json::json!({ "path": path.to_str().unwrap() })
}

#[tokio::test]
async fn test_parse_legacy_doc_fixture() {
    let mut temp_file = NamedTempFile::with_suffix(".doc").unwrap();
    temp_file.write_all(LEGACY_DOC_BYTES).unwrap();
    temp_file.flush().unwrap();

    let tool = DocParserTool;
    let context = ToolExecutionContext::new(Uuid::new_v4());

    let result = tool
        .execute(parse_input(temp_file.path()), &context)
        .await
        .unwrap();
    assert!(result.success);
    assert!(result.output.contains("Containing block before"));
    assert!(result.output.contains("Containing block after"));
}

#[tokio::test]
async fn test_parse_garbage_legacy_doc_errors_cleanly() {
    let mut temp_file = NamedTempFile::with_suffix(".doc").unwrap();
    writeln!(temp_file, "this is not an OLE compound document").unwrap();
    temp_file.flush().unwrap();

    let tool = DocParserTool;
    let context = ToolExecutionContext::new(Uuid::new_v4());

    let err = tool
        .execute(parse_input(temp_file.path()), &context)
        .await
        .unwrap_err();
    assert!(format!("{:?}", err).contains("Failed to parse legacy DOC"));
}

#[tokio::test]
async fn test_parse_xlsm_routed_to_xlsx_reader() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("macro_workbook.xlsm");

    let mut workbook = rust_xlsxwriter::Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_string(0, 0, "MacroData").unwrap();
    sheet.write_string(1, 0, "legacy-routing-955").unwrap();
    let mut buf = Vec::new();
    workbook.save_to_writer(&mut buf).unwrap();
    std::fs::write(&path, &buf).unwrap();

    let tool = DocParserTool;
    let context = ToolExecutionContext::new(Uuid::new_v4());

    let result = tool.execute(parse_input(&path), &context).await.unwrap();
    assert!(result.success);
    assert!(result.output.contains("=== Sheet:"));
    assert!(result.output.contains("MacroData"));
    assert!(result.output.contains("legacy-routing-955"));
}

#[tokio::test]
async fn test_parse_garbage_xlsb_errors_cleanly() {
    let mut temp_file = NamedTempFile::with_suffix(".xlsb").unwrap();
    writeln!(temp_file, "not a real binary workbook").unwrap();
    temp_file.flush().unwrap();

    let tool = DocParserTool;
    let context = ToolExecutionContext::new(Uuid::new_v4());

    let err = tool
        .execute(parse_input(temp_file.path()), &context)
        .await
        .unwrap_err();
    assert!(format!("{:?}", err).contains("Failed to open XLSB"));
}

#[tokio::test]
async fn test_parse_garbage_ods_errors_cleanly() {
    let mut temp_file = NamedTempFile::with_suffix(".ods").unwrap();
    writeln!(temp_file, "not a real ods archive").unwrap();
    temp_file.flush().unwrap();

    let tool = DocParserTool;
    let context = ToolExecutionContext::new(Uuid::new_v4());

    let err = tool
        .execute(parse_input(temp_file.path()), &context)
        .await
        .unwrap_err();
    assert!(format!("{:?}", err).contains("Failed to open ODS"));
}

#[tokio::test]
async fn test_unsupported_format_message_lists_legacy_formats() {
    let mut temp_file = NamedTempFile::with_suffix(".xyz").unwrap();
    writeln!(temp_file, "Some content").unwrap();
    temp_file.flush().unwrap();

    let tool = DocParserTool;
    let context = ToolExecutionContext::new(Uuid::new_v4());

    let result = tool
        .execute(parse_input(temp_file.path()), &context)
        .await
        .unwrap();
    assert!(!result.success);
    let error = result.error.unwrap();
    assert!(error.contains("DOC"));
    assert!(error.contains("XLSB"));
    assert!(error.contains("ODS"));
}
