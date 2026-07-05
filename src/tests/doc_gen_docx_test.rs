//! Tests for the `generate_document` DOCX backend (#357).
//!
//! Generated documents are unzipped (a .docx is an OOXML zip) and the
//! word/document.xml payload is asserted directly, so the round trip proves
//! the archive is well-formed and the content actually landed.

use crate::brain::tools::doc_gen::docx::{BlockSpec, DocxStyle, write_document};
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
        &DocxStyle::default(),
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
        &DocxStyle::default(),
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
        &DocxStyle::default(),
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
        &DocxStyle::default(),
    )
    .expect("document written");
    let xml = document_xml(&path);
    assert!(xml.contains("true"));
    assert!(xml.contains("k"));
}

#[test]
fn styled_document_carries_colors_furniture_and_shading() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("styled.docx");
    let style: DocxStyle = serde_json::from_value(json!({
        "accent_color": "#0A84FF",
        "page_header": "OpenCrabs Research",
        "page_footer": "confidential",
        "table_header_fill": "#0A84FF",
        "zebra_rows": true
    }))
    .expect("style parses");
    write_document(
        &path,
        &blocks(json!([
            {"type": "heading", "text": "Branded", "level": 1},
            {"type": "table", "rows": [["H1", "H2"], ["a", "b"], ["c", "d"]]}
        ])),
        &style,
    )
    .expect("styled document written");

    let file = std::fs::File::open(&path).expect("docx opens");
    let mut archive = zip::ZipArchive::new(file).expect("docx is a zip");
    let mut styles = String::new();
    archive
        .by_name("word/styles.xml")
        .expect("styles part")
        .read_to_string(&mut styles)
        .expect("styles xml");
    assert!(styles.contains("0A84FF"), "accent on heading styles");

    let mut doc = String::new();
    archive
        .by_name("word/document.xml")
        .expect("document part")
        .read_to_string(&mut doc)
        .expect("document xml");
    assert!(doc.contains("0A84FF"), "header row shading present");
    assert!(doc.contains("F2F2F2"), "zebra shading present");

    let mut header = String::new();
    archive
        .by_name("word/header1.xml")
        .expect("header part present")
        .read_to_string(&mut header)
        .expect("header xml");
    assert!(header.contains("OpenCrabs Research"));

    let mut footer = String::new();
    archive
        .by_name("word/footer1.xml")
        .expect("footer part present")
        .read_to_string(&mut footer)
        .expect("footer xml");
    assert!(footer.contains("confidential"));
}
