//! Issue #91 guardrail: protected brain files cannot be mutated via
//! the generic `write_file` / `edit_file` tools. They must go through
//! `write_opencrabs_file`, which enforces append-only writes,
//! dedup-aware shrinking, and `.bak` snapshots.
//!
//! Tests pin both the rejection (path under any directory ending in a
//! protected brain file name) and the pass-through (non-brain files
//! still write normally, even inside `~/.opencrabs/`).

use crate::brain::tools::edit::EditTool;
use crate::brain::tools::write::WriteTool;
use crate::brain::tools::{Tool, ToolExecutionContext};
use serde_json::json;
use tempfile::TempDir;
use uuid::Uuid;

/// write_file refuses to overwrite a protected brain file even when
/// the caller passes an absolute path that bypasses working-dir
/// resolution. Error text must point the caller at write_opencrabs_file.
#[tokio::test]
async fn write_file_rejects_protected_brain_file() {
    let temp = TempDir::new().unwrap();
    let brain_path = temp.path().join("MEMORY.md");
    std::fs::write(&brain_path, "original brain content").unwrap();

    let tool = WriteTool;
    let context =
        ToolExecutionContext::new(Uuid::new_v4()).with_working_directory(temp.path().to_path_buf());

    let result = tool
        .execute(
            json!({
                "path": brain_path.to_str().unwrap(),
                "content": ""
            }),
            &context,
        )
        .await
        .unwrap();

    assert!(!result.success, "write_file must refuse protected brain file");
    let err = result.error.unwrap_or_default();
    assert!(
        err.contains("write_opencrabs_file"),
        "error must route caller to write_opencrabs_file, got: {err}"
    );
    let still_there = std::fs::read_to_string(&brain_path).unwrap();
    assert_eq!(
        still_there, "original brain content",
        "brain file content must be untouched after rejection"
    );
}

/// edit_file is the other generic-write path. Same guardrail applies.
#[tokio::test]
async fn edit_file_rejects_protected_brain_file() {
    let temp = TempDir::new().unwrap();
    let brain_path = temp.path().join("SOUL.md");
    std::fs::write(&brain_path, "original soul content\nline two").unwrap();

    let tool = EditTool;
    let context =
        ToolExecutionContext::new(Uuid::new_v4()).with_working_directory(temp.path().to_path_buf());

    let result = tool
        .execute(
            json!({
                "path": brain_path.to_str().unwrap(),
                "operation": "replace",
                "old_text": "original soul content",
                "new_text": ""
            }),
            &context,
        )
        .await
        .unwrap();

    assert!(!result.success, "edit_file must refuse protected brain file");
    let err = result.error.unwrap_or_default();
    assert!(
        err.contains("write_opencrabs_file"),
        "error must route caller to write_opencrabs_file, got: {err}"
    );
    let still_there = std::fs::read_to_string(&brain_path).unwrap();
    assert_eq!(
        still_there, "original soul content\nline two",
        "brain file content must be untouched after rejection"
    );
}

/// Non-brain files inside the same dir are still writable. The guard
/// is name-based, not directory-based, so legitimate uses (memory logs,
/// commands.toml, etc.) keep working.
#[tokio::test]
async fn write_file_allows_non_brain_files_in_same_dir() {
    let temp = TempDir::new().unwrap();
    let non_brain_path = temp.path().join("notes.md");

    let tool = WriteTool;
    let context =
        ToolExecutionContext::new(Uuid::new_v4()).with_working_directory(temp.path().to_path_buf());

    let result = tool
        .execute(
            json!({
                "path": non_brain_path.to_str().unwrap(),
                "content": "scratch notes"
            }),
            &context,
        )
        .await
        .unwrap();

    assert!(result.success, "non-brain files must still write normally");
    let written = std::fs::read_to_string(&non_brain_path).unwrap();
    assert_eq!(written, "scratch notes");
}

/// Sanity check the guard covers every name in the protected list, so a
/// future addition to `brain_file_safety::PROTECTED_BRAIN_FILES`
/// automatically picks up the same generic-tool protection.
#[tokio::test]
async fn write_file_rejects_every_protected_brain_file() {
    let names = [
        "SOUL.md",
        "USER.md",
        "AGENTS.md",
        "TOOLS.md",
        "CODE.md",
        "SECURITY.md",
        "MEMORY.md",
        "BOOT.md",
        "IDENTITY.md",
    ];

    for name in names {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join(name);
        std::fs::write(&path, "x").unwrap();

        let tool = WriteTool;
        let context = ToolExecutionContext::new(Uuid::new_v4())
            .with_working_directory(temp.path().to_path_buf());

        let result = tool
            .execute(
                json!({ "path": path.to_str().unwrap(), "content": "y" }),
                &context,
            )
            .await
            .unwrap();

        assert!(!result.success, "write_file must refuse {name}");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "x",
            "{name} content must survive rejection"
        );
    }
}
