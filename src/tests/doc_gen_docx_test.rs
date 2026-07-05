//! Tests for the `generate_document` DOCX backend (#357).
//!
//! Generated documents are unzipped (a .docx is an OOXML zip) and the
//! word/document.xml payload is asserted directly, so the round trip proves
//! the archive is well-formed and the content actually landed.

use crate::brain::tools::doc_gen::docx::{BlockSpec, write_document};
use serde_json::json;
use std::io::Read;

fn blocks(v: serde_json::Value) -> Vec<BlockSpec> {
    serde_json::from_value(v).expect("valid block specs")
}

fn document_xml(path: &std::path::Path) -> String {
    let file = std::fs::File::open(path).expect("docx opens");
    let mut archive = zip::ZipArchive::new(file).expect("docx is a zip");
    let mut entry = archive
        .by_name("word/document.xml")
        .expect("document.xml present");
    let mut xml = String::new();
    entry.read_to_string(&mut xml).expect("xml reads");
    xml
}

#[test]
fn headings_paragraphs_lists_and_tables_land() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("report.docx");
    let summary = write_document(
        &path,
        &blocks(json!([
            {"type": "heading", "text": "Quarterly Report", "level": 1},
            {"type": "paragraph", "text": "Everything is on track."},
            {"type": "list", "items": ["first point", "second point"]},
            {"type": "table", "rows": [["Metric", "Value"], ["Uptime", 99.9]]}
        ])),
    )
    .expect("document written");
    assert!(summary.contains("1 heading(s)"));
    assert!(summary.contains("1 paragraph(s)"));
    assert!(summary.contains("1 list(s)"));
    assert!(summary.contains("1 table(s)"));

    let xml = document_xml(&path);
    assert!(xml.contains("Quarterly Report"));
    assert!(xml.contains("Everything is on track."));
    assert!(xml.contains("first point"));
    assert!(xml.contains("Uptime"));
    assert!(xml.contains("99.9"));
    // Heading uses a real Word paragraph style, not just a big bold run.
    assert!(xml.contains("Heading1"));
    // Table is a real Word table.
    assert!(xml.contains("<w:tbl>"));
}

#[test]
fn lists_use_real_numbering_definitions() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("lists.docx");
    write_document(
        &path,
        &blocks(json!([
            {"type": "list", "items": ["alpha"], "ordered": false},
            {"type": "list", "items": ["beta"], "ordered": true}
        ])),
    )
    .expect("document written");

    let xml = document_xml(&path);
    // Both list paragraphs reference numbering ids, resolved by the
    // numbering part packed into the archive.
    assert!(xml.contains("<w:numPr>"));
    let file = std::fs::File::open(&path).expect("docx opens");
    let mut archive = zip::ZipArchive::new(file).expect("docx is a zip");
    let mut numbering = String::new();
    archive
        .by_name("word/numbering.xml")
        .expect("numbering part present")
        .read_to_string(&mut numbering)
        .expect("numbering reads");
    assert!(numbering.contains("bullet"));
    assert!(numbering.contains("decimal"));
}

#[test]
fn heading_level_is_clamped_not_fatal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("clamp.docx");
    write_document(
        &path,
        &blocks(json!([{"type": "heading", "text": "Deep", "level": 9}])),
    )
    .expect("out-of-range level still writes");
    let xml = document_xml(&path);
    assert!(xml.contains("Heading3"));
}

#[test]
fn table_cells_stringify_non_string_json() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("cells.docx");
    write_document(
        &path,
        &blocks(json!([
            {"type": "table", "rows": [[true, {"k": 1}, null]], "header_bold": false}
        ])),
    )
    .expect("document written");
    let xml = document_xml(&path);
    assert!(xml.contains("true"));
    assert!(xml.contains("k"));
}
