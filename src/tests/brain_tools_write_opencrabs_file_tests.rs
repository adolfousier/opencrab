use crate::brain::tools::Tool;
// Tests for `write_opencrabs_file` tool.

use crate::brain::tools::write_opencrabs_file::*;

use crate::brain::tools::ToolExecutionContext;
use crate::brain::tools::write_opencrabs_file::validate_opencrabs_path;
use tempfile::TempDir;
use uuid::Uuid;

fn ctx() -> ToolExecutionContext {
    ToolExecutionContext::new(Uuid::new_v4())
}

fn tool() -> WriteOpenCrabsFileTool {
    WriteOpenCrabsFileTool
}

// ── metadata ─────────────────────────────────────────────────────────────────

#[test]
fn test_tool_name_and_requires_approval() {
    let t = tool();
    assert_eq!(t.name(), "write_opencrabs_file");
    assert!(t.requires_approval(), "writes must require approval");
}

#[test]
fn test_input_schema_required_fields() {
    let schema = tool().input_schema();
    let required = schema["required"].as_array().unwrap();
    assert!(required.iter().any(|v| v.as_str() == Some("path")));
    assert!(required.iter().any(|v| v.as_str() == Some("operation")));
}

// ── path validation ───────────────────────────────────────────────────────────

#[test]
fn test_empty_path_rejected() {
    assert!(validate_opencrabs_path("").is_err());
}

#[test]
fn test_absolute_path_rejected() {
    assert!(validate_opencrabs_path("/etc/passwd").is_err());
    assert!(validate_opencrabs_path("~/MEMORY.md").is_err());
}

#[test]
fn test_path_traversal_rejected() {
    assert!(validate_opencrabs_path("../etc/passwd").is_err());
    assert!(validate_opencrabs_path("../../secrets").is_err());
    assert!(validate_opencrabs_path("subdir/../../etc/passwd").is_err());
}

#[test]
fn test_valid_brain_files_accepted() {
    assert!(validate_opencrabs_path("MEMORY.md").is_ok());
    assert!(validate_opencrabs_path("USER.md").is_ok());
    assert!(validate_opencrabs_path("SOUL.md").is_ok());
}

#[test]
fn test_valid_config_files_accepted() {
    assert!(validate_opencrabs_path("commands.toml").is_ok());
    assert!(validate_opencrabs_path("config.toml").is_ok());
}

#[test]
fn test_valid_subdirectory_paths_accepted() {
    assert!(validate_opencrabs_path("memory/2026-03-02.md").is_ok());
    assert!(validate_opencrabs_path("agents/session/context.json").is_ok());
}

#[test]
fn test_profiles_prefix_rejected() {
    let result = validate_opencrabs_path("profiles/ops/TOOLS.md");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("directory prefix"),
        "Error message should mention directory prefix: {err}"
    );
    assert!(
        err.contains("profiles/ops/TOOLS.md"),
        "Error should echo the bad path: {err}"
    );

    // Also reject just "profiles/" with nothing after
    let result2 = validate_opencrabs_path("profiles/ops/MEMORY.md");
    assert!(result2.is_err());
}

// ── operation validation ──────────────────────────────────────────────────────

#[tokio::test]
async fn test_unknown_operation_returns_error() {
    let result = tool()
        .execute(
            serde_json::json!({"path": "MEMORY.md", "operation": "delete"}),
            &ctx(),
        )
        .await
        .unwrap();
    assert!(!result.success);
    assert!(result.error.unwrap().contains("Unknown operation"));
}

#[tokio::test]
async fn test_overwrite_missing_content_returns_error() {
    let result = tool()
        .execute(
            serde_json::json!({"path": "MEMORY.md", "operation": "overwrite"}),
            &ctx(),
        )
        .await
        .unwrap();
    assert!(!result.success);
    assert!(result.error.unwrap().contains("content is required"));
}

#[tokio::test]
async fn test_append_missing_content_returns_error() {
    let result = tool()
        .execute(
            serde_json::json!({"path": "MEMORY.md", "operation": "append"}),
            &ctx(),
        )
        .await
        .unwrap();
    assert!(!result.success);
    assert!(result.error.unwrap().contains("content is required"));
}

#[tokio::test]
async fn test_replace_missing_old_text_returns_error() {
    let result = tool()
        .execute(
            serde_json::json!({"path": "MEMORY.md", "operation": "replace", "new_text": "x"}),
            &ctx(),
        )
        .await
        .unwrap();
    assert!(!result.success);
    assert!(result.error.unwrap().contains("old_text is required"));
}

#[tokio::test]
async fn test_replace_missing_new_text_returns_error() {
    let result = tool()
        .execute(
            serde_json::json!({"path": "MEMORY.md", "operation": "replace", "old_text": "x"}),
            &ctx(),
        )
        .await
        .unwrap();
    assert!(!result.success);
    assert!(result.error.unwrap().contains("new_text is required"));
}

// ── roundtrip write + read ────────────────────────────────────────────────────

#[test]
fn test_overwrite_roundtrip_via_fs() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("MEMORY.md");
    std::fs::write(&path, "initial content").unwrap();
    std::fs::write(&path, "updated content").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "updated content");
}

#[test]
fn test_append_roundtrip_via_fs() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("MEMORY.md");
    std::fs::write(&path, "## Section\n").unwrap();
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    f.write_all(b"\nnew note").unwrap();
    let result = std::fs::read_to_string(&path).unwrap();
    assert!(result.contains("## Section"));
    assert!(result.contains("new note"));
}

#[test]
fn test_replace_roundtrip_via_fs() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("MEMORY.md");
    std::fs::write(&path, "old value here").unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    let updated = content.replacen("old value", "new value", 1);
    std::fs::write(&path, &updated).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new value here");
}

#[test]
fn test_subdirectory_created_on_write() {
    let dir = TempDir::new().unwrap();
    let subdir = dir.path().join("memory");
    let path = subdir.join("note.md");
    // Simulate create_dir_all + write
    std::fs::create_dir_all(&subdir).unwrap();
    std::fs::write(&path, "content").unwrap();
    assert!(path.exists());
}

// ── NFC normalization for replace ────────────────────────────────────────────

/// Verify that NFC normalization makes replace work when the file has NFD
/// bytes but the search string uses NFC (the common macOS copy-paste case).
/// Regression test for issue #256.
#[test]
fn test_replace_nfc_normalizes_both_sides() {
    use unicode_normalization::UnicodeNormalization;

    let dir = TempDir::new().unwrap();
    let path = dir.path().join("MEMORY.md");

    // Em-dash: U+2014. In NFC it's 3 bytes (E2 80 94). In NFD it's the same
    // (em-dash has no decomposition). Multiplication sign: U+00D7.
    // In NFC: C3 97. In NFD: same (no decomposition).
    // The real-world issue is with accented characters, e.g. é:
    // NFC: C3 A9 (single codepoint U+00E9)
    // NFD: 65 CC 81 (e + combining acute accent U+0301)

    // Write file with NFD-normalized content (as macOS might produce)
    let nfd_content: String = "café résumé".nfd().collect();
    std::fs::write(&path, &nfd_content).unwrap();

    // Search with NFC-normalized text (as most users would type)
    let search_nfc: String = "café".nfc().collect();
    let replace_nfc: String = "coffee".nfc().collect();

    // Verify they're different byte sequences
    let search_nfd: String = search_nfc.nfd().collect();
    assert_ne!(
        search_nfc.as_bytes(),
        search_nfd.as_bytes(),
        "NFC and NFD should differ for accented chars"
    );

    // Raw contains() would FAIL because NFC search won't match NFD file
    let raw_file = std::fs::read_to_string(&path).unwrap();
    assert!(
        !raw_file.contains(search_nfc.as_str()),
        "raw contains should fail with mixed normalization"
    );

    // NFC-normalize both sides (what write_opencrabs_file now does)
    let file_nfc: String = raw_file.nfc().collect();
    let search_nfc_norm: String = search_nfc.nfc().collect();
    assert!(
        file_nfc.contains(search_nfc_norm.as_str()),
        "NFC-normalized contains should match"
    );

    // Do the replacement
    let updated = file_nfc.replacen(search_nfc_norm.as_str(), replace_nfc.as_str(), 1);
    std::fs::write(&path, &updated).unwrap();

    let result = std::fs::read_to_string(&path).unwrap();
    assert!(
        result.contains("coffee"),
        "replacement should have succeeded: got {:?}",
        result
    );
    assert!(
        result.contains("résumé"),
        "unrelated text should be preserved: got {:?}",
        result
    );
}

/// Verify that replace with Unicode symbols (em-dash, multiplication sign,
/// less-than-or-equal) works through NFC normalization.
#[test]
fn test_replace_unicode_symbols_via_nfc() {
    use unicode_normalization::UnicodeNormalization;

    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.md");

    // Content with various Unicode symbols
    let content = "price: 10×2 ≤ 25 — final";
    std::fs::write(&path, content).unwrap();

    let search = "10×2";
    let replacement = "10x2";

    // NFC normalize both sides
    let raw = std::fs::read_to_string(&path).unwrap();
    let file_nfc: String = raw.nfc().collect();
    let search_nfc: String = search.nfc().collect();

    assert!(
        file_nfc.contains(search_nfc.as_str()),
        "multiplication sign should match after NFC normalization"
    );

    let updated = file_nfc.replacen(search_nfc.as_str(), replacement, 1);
    std::fs::write(&path, &updated).unwrap();

    let result = std::fs::read_to_string(&path).unwrap();
    assert!(result.contains("10x2"), "replacement succeeded: {:?}", result);
    assert!(result.contains("≤"), "other Unicode preserved: {:?}", result);
    assert!(result.contains("—"), "em-dash preserved: {:?}", result);
}
