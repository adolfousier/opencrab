//! Tests for `App::extract_attachments` drag-drop handling: terminals
//! shell-escape dropped paths (spaces become `\ `), which used to fail the
//! `Path::exists()` check so a dropped screenshot silently never attached.
//! Also covers PDF/doc paths surfacing a parse hint rather than vanishing.

use crate::tui::app::App;

#[test]
fn escaped_space_image_path_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("Screenshot 1.png");
    std::fs::write(&real, b"\x89PNG\r\n").unwrap();
    // What the terminal pastes on drag-drop: spaces backslash-escaped.
    let escaped = real.display().to_string().replace(' ', "\\ ");

    let e = App::extract_attachments(&escaped);

    let (clean, atts) = (e.text, e.attachments);
    assert_eq!(atts.len(), 1, "escaped-space image path should attach");
    assert_eq!(atts[0].path, real.to_string_lossy());
    assert!(!atts[0].is_video);
    assert!(
        clean.is_empty(),
        "the path is consumed, leaving no stray text"
    );
}

#[test]
fn plain_image_path_without_spaces_still_works() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("plain.png");
    std::fs::write(&real, b"\x89PNG\r\n").unwrap();

    let e = App::extract_attachments(real.to_str().unwrap());

    let (_clean, atts) = (e.text, e.attachments);
    assert_eq!(atts.len(), 1);
    assert_eq!(atts[0].path, real.to_string_lossy());
}

#[test]
fn pdf_path_surfaces_parse_hint_not_attachment() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("report 2026.pdf");
    std::fs::write(&real, b"%PDF-1.4\n").unwrap();
    let escaped = real.display().to_string().replace(' ', "\\ ");

    let e = App::extract_attachments(&escaped);

    let (text, atts) = (e.text, e.attachments);
    assert!(atts.is_empty(), "a PDF is not an image attachment");
    assert!(
        text.contains("pdf_to_images") && text.contains("parse_document"),
        "doc path should hint the parsing tools: {text}"
    );
    assert!(
        text.contains(&real.to_string_lossy().to_string()),
        "doc hint should carry the real path: {text}"
    );
}

#[test]
fn nonexistent_escaped_path_is_left_as_text() {
    // No file on disk → nothing attached, text preserved (not silently eaten).
    let e = App::extract_attachments("/no/such/Screenshot\\ 1.png");
    let (text, atts) = (e.text, e.attachments);
    assert!(atts.is_empty());
    assert!(text.contains("Screenshot"));
}
