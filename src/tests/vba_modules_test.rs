//! VBA macro source extraction from macro-enabled workbooks (#960).
//!
//! `src/tests/fixtures/vbaProject.bin` is vendored from the `rust_xlsxwriter`
//! crate's examples directory (MIT OR Apache-2.0), the same crate this repo
//! already depends on for writing workbooks. It has to be a genuine compiled
//! VBA project: calamine parses the CFB container and its compressed module
//! streams, so a synthetic blob does not survive `VbaProject::new`.
//!
//! The .xlsm itself is built at test time rather than committed, so the only
//! binary in the tree is the 17 KB project blob.

use crate::brain::tools::Tool;
use crate::brain::tools::ToolExecutionContext;
use crate::brain::tools::doc_parser::DocParserTool;
use crate::brain::tools::vba_modules::{VbaModule, clip, extract, render};
use calamine::{Xlsx, open_workbook};
use rust_xlsxwriter::Workbook;
use std::path::Path;
use uuid::Uuid;

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/tests/fixtures/vbaProject.bin"
);

fn module(name: &str, source: &str) -> VbaModule {
    VbaModule {
        name: name.to_string(),
        source: source.to_string(),
    }
}

/// Write a one-sheet workbook, optionally carrying the fixture's VBA project.
fn write_workbook(path: &Path, with_macros: bool) {
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_string(0, 0, "value").expect("write cell");
    if with_macros {
        workbook
            .add_vba_project(FIXTURE)
            .expect("attach vba project");
    }
    workbook.save(path).expect("save workbook");
}

#[test]
fn clip_leaves_a_short_source_alone() {
    let (body, clipped) = clip("Sub Demo()\nEnd Sub", 1024);
    assert_eq!(body, "Sub Demo()\nEnd Sub");
    assert!(!clipped);
}

#[test]
fn clip_steps_back_to_a_char_boundary() {
    // "é" is two bytes, so a cut at byte 3 lands mid-character.
    let (body, clipped) = clip("ab\u{e9}cd", 3);
    assert_eq!(body, "ab");
    assert!(clipped);
}

#[test]
fn render_of_no_modules_is_empty() {
    // A macro-free workbook must produce byte-identical output to before the
    // feature existed, so this returns "" rather than an empty header.
    assert_eq!(render(&[]), "");
}

#[test]
fn render_lists_every_name_then_each_body() {
    let out = render(&[
        module("Module1", "Sub Alpha()\nEnd Sub"),
        module("Sheet1", "Sub Beta()\nEnd Sub"),
    ]);

    assert!(out.contains("=== VBA modules ==="));
    assert!(out.contains("- Module1\n"));
    assert!(out.contains("- Sheet1\n"));
    assert!(out.contains("--- Module: Module1 ---"));
    assert!(out.contains("Sub Alpha()"));
    assert!(out.contains("Sub Beta()"));
}

#[test]
fn render_marks_a_module_it_had_to_truncate() {
    let out = render(&[module("Fat", &"x".repeat(40 * 1024))]);
    assert!(
        out.contains("[truncated]"),
        "no truncation marker: {out:.200}"
    );
    assert!(out.len() < 40 * 1024, "body was not clipped");
}

#[test]
fn render_stops_once_the_total_budget_is_spent() {
    // Five 40 KB modules clip to 32 KB each, so the 128 KB total ceiling is
    // gone before the last one and it must be named but not dumped.
    let big = "y".repeat(40 * 1024);
    let modules: Vec<VbaModule> = (0..5).map(|i| module(&format!("M{i}"), &big)).collect();

    let out = render(&modules);

    assert!(
        out.contains("- M4\n"),
        "every module should still be listed"
    );
    assert!(out.contains("--- Module: M4 ---"));
    assert!(out.contains("[omitted: total VBA size limit reached]"));
}

#[test]
fn a_macro_free_workbook_yields_no_modules() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("plain.xlsx");
    write_workbook(&path, false);

    let mut workbook: Xlsx<_> = open_workbook(&path).expect("open xlsx");
    assert!(extract(&mut workbook).is_empty());
}

#[test]
fn a_macro_workbook_yields_decompressed_source() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("macros.xlsm");
    write_workbook(&path, true);

    let mut workbook: Xlsx<_> = open_workbook(&path).expect("open xlsm");
    let modules = extract(&mut workbook);

    assert!(
        !modules.is_empty(),
        "fixture project has no readable modules"
    );
    let joined: String = modules.iter().map(|m| m.source.as_str()).collect();
    assert!(
        joined.contains("Sub ") || joined.contains("Function "),
        "no VBA procedure in extracted source: {:?}",
        modules.iter().map(|m| &m.name).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn parse_document_appends_macro_source_for_xlsm() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("macros.xlsm");
    write_workbook(&path, true);

    let context = ToolExecutionContext::new(Uuid::new_v4());
    let input = serde_json::json!({ "path": path.to_str().expect("utf8 path") });
    let result = DocParserTool
        .execute(input, &context)
        .await
        .expect("parse xlsm");

    assert!(result.success);
    assert!(result.output.contains("=== Sheet:"), "sheet dump missing");
    assert!(
        result.output.contains("=== VBA modules ==="),
        "macro section missing from parse_document output"
    );
}

#[tokio::test]
async fn parse_document_stays_silent_for_a_macro_free_workbook() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("plain.xlsx");
    write_workbook(&path, false);

    let context = ToolExecutionContext::new(Uuid::new_v4());
    let input = serde_json::json!({ "path": path.to_str().expect("utf8 path") });
    let result = DocParserTool
        .execute(input, &context)
        .await
        .expect("parse xlsx");

    assert!(result.success);
    assert!(!result.output.contains("VBA modules"));
}
