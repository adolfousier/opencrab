//! Tests for slack_send's upload content-type inference (#370).
//!
//! The deprecated files.upload path hardcoded image/png for every file,
//! which broke PDF/doc delivery. The external upload flow posts bytes with
//! a real MIME type inferred from the extension.

use crate::brain::tools::slack_send::content_type_for;

#[test]
fn common_document_types_map_correctly() {
    assert_eq!(content_type_for("report.pdf"), "application/pdf");
    assert_eq!(
        content_type_for("sheet.xlsx"),
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
    );
    assert_eq!(
        content_type_for("doc.docx"),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    );
    assert_eq!(content_type_for("logo.png"), "image/png");
    assert_eq!(content_type_for("PHOTO.JPG"), "image/jpeg");
}

#[test]
fn unknown_and_missing_extensions_fall_back_to_octet_stream() {
    assert_eq!(content_type_for("file.weird"), "application/octet-stream");
    assert_eq!(content_type_for("noext"), "application/octet-stream");
}
