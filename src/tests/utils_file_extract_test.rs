use super::*;

#[test]
fn collapse_double_extension_handles_double_docx() {
    assert_eq!(
        collapse_double_extension("zaiavlenie_s_ekzamena.docx.docx"),
        "zaiavlenie_s_ekzamena.docx"
    );
}

#[test]
fn collapse_double_extension_handles_case_insensitive() {
    assert_eq!(collapse_double_extension("file.DOCX.DOCX"), "file.DOCX");
    assert_eq!(collapse_double_extension("FILE.Pdf.Pdf"), "FILE.Pdf");
}

#[test]
fn collapse_double_extension_passthrough_no_double() {
    assert_eq!(collapse_double_extension("file.docx"), "file.docx");
    assert_eq!(collapse_double_extension("file.doc.pdf"), "file.doc.pdf");
    assert_eq!(collapse_double_extension("file"), "file");
}

#[test]
fn collapse_double_extension_short() {
    assert_eq!(collapse_double_extension("a.docx.docx"), "a.docx");
    assert_eq!(collapse_double_extension("a.b.c"), "a.b.c");
}

#[test]
fn collapse_double_extension_single_char_extension() {
    // Edge case: single-char inner ext same as outer (e.g. "file.x.x")
    assert_eq!(collapse_double_extension("file.x.x"), "file.x");
}
