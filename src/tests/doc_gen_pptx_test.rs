//! Tests for the `generate_document` PPTX fallback backend (#357).
//!
//! PPTX generation dispatches to the host's python-pptx. The end-to-end
//! test runs only where that stack exists (it self-skips otherwise, since
//! the dependency is environmental); the error-path tests always run.

use crate::brain::tools::doc_gen::pptx::{SlideSpec, write_deck};
use serde_json::json;

fn slides(v: serde_json::Value) -> Vec<SlideSpec> {
    serde_json::from_value(v).expect("valid slide specs")
}

fn host_has_python_pptx() -> bool {
    std::process::Command::new("python3")
        .args(["-c", "import pptx"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[tokio::test]
async fn deck_generates_when_python_pptx_present() {
    if !host_has_python_pptx() {
        eprintln!("skipping: python3/python-pptx not available on this host");
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("deck.pptx");
    let summary = write_deck(
        &path,
        &slides(json!([
            {"title": "Kickoff", "bullets": ["goal one", "goal two"], "notes": "greet everyone"},
            {"title": "Roadmap"}
        ])),
    )
    .await
    .expect("deck written");
    assert!(summary.contains("2 slide(s)"));
    assert!(path.exists());
    // A .pptx is an OOXML zip; the first slide's XML must carry the content.
    let file = std::fs::File::open(&path).expect("pptx opens");
    let mut archive = zip::ZipArchive::new(file).expect("pptx is a zip");
    let mut xml = String::new();
    use std::io::Read;
    archive
        .by_name("ppt/slides/slide1.xml")
        .expect("slide present")
        .read_to_string(&mut xml)
        .expect("slide xml reads");
    assert!(xml.contains("Kickoff"));
    assert!(xml.contains("goal one"));
}

#[tokio::test]
async fn missing_python_pptx_reports_actionable_error() {
    if host_has_python_pptx() {
        eprintln!("skipping: python-pptx IS available, cannot exercise the missing-module path");
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("deck.pptx");
    let err = write_deck(&path, &slides(json!([{"title": "T"}])))
        .await
        .expect_err("must fail without python-pptx");
    assert!(
        err.contains("python-pptx"),
        "error must name the missing dependency: {err}"
    );
    assert!(
        err.contains("pip3 install python-pptx"),
        "error must tell the user how to fix it: {err}"
    );
}

#[test]
fn slide_titles_with_quotes_serialize_safely() {
    // Content travels as JSON over stdin, never interpolated into the
    // Python script, so quoting cannot break out of the generator.
    let specs = slides(json!([
        {"title": "He said \"hello\"; import os", "bullets": ["'); print('x"]}
    ]));
    let payload = serde_json::json!({ "slides": specs }).to_string();
    let parsed: serde_json::Value = serde_json::from_str(&payload).expect("round trips");
    assert_eq!(parsed["slides"][0]["title"], "He said \"hello\"; import os");
}
