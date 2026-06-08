//! Tests for `telegram_send::resolve_input_file` — the helper that lets
//! `send_photo` / `send_document` accept either an HTTPS URL or a local file
//! path. Before this, both actions passed the reference straight to
//! `InputFile::url()`, so any local path was rejected as an invalid URL (#181).
//!
//! `InputFile` is opaque (no public accessors), so these tests assert on the
//! observable contract: which references resolve to `Ok` and which produce a
//! `ToolResult::error`, plus that the error message is descriptive.

use crate::brain::tools::telegram_send::resolve_input_file;

#[tokio::test]
async fn https_url_resolves_ok() {
    let result = resolve_input_file("https://example.com/cat.png", "photo_url").await;
    assert!(result.is_ok(), "an HTTPS URL must resolve to an InputFile");
}

#[tokio::test]
async fn http_url_resolves_ok() {
    let result = resolve_input_file("http://example.com/doc.pdf", "document_url").await;
    assert!(
        result.is_ok(),
        "a plain HTTP URL must resolve to an InputFile"
    );
}

#[tokio::test]
async fn existing_local_file_resolves_ok() {
    // Write a temp file, then confirm the helper reads it into memory.
    let dir = std::env::temp_dir();
    let path = dir.join("opencrabs_resolve_input_file_test.bin");
    tokio::fs::write(&path, b"hello bytes").await.unwrap();

    let result = resolve_input_file(&path.to_string_lossy(), "document_url").await;
    assert!(
        result.is_ok(),
        "an existing local path must resolve to an in-memory InputFile"
    );

    let _ = tokio::fs::remove_file(&path).await;
}

#[tokio::test]
async fn missing_local_file_returns_error() {
    let result = resolve_input_file(
        "/tmp/opencrabs_definitely_missing_file_12345.png",
        "photo_url",
    )
    .await;
    let err = result.expect_err("a missing local path must produce an error");
    assert!(!err.success);
    let msg = err.error.unwrap_or_default();
    assert!(
        msg.contains("Failed to read local photo_url"),
        "error should name the failing reference, got: {msg}"
    );
}

#[tokio::test]
async fn invalid_url_scheme_treated_as_local_path() {
    // Not http(s):// → treated as a local path. "ftp://..." isn't a real file,
    // so it must surface a read error rather than being passed to InputFile::url.
    let result = resolve_input_file("ftp://example.com/file.bin", "document_url").await;
    let err = result.expect_err("a non-http(s) reference must be read as a local path");
    assert!(
        err.error
            .unwrap_or_default()
            .contains("Failed to read local")
    );
}
