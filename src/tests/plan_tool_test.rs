//! Tests for the plan tool — security hardening + import operation.
//!
//! Originally lived inline at
//! `src/brain/tools/plan_tool_security_tests.rs` as a
//! `#[cfg(test)] mod tests { ... }` submodule of `plan_tool`. Moved
//! here as part of PR #160's review — the project convention is that
//! every test is a top-level file under `src/tests/` registered in
//! `tests/mod.rs`, no inline `#[cfg(test)] mod tests` blocks anywhere
//! else in the tree. Items the tests touch
//! (`validate_plan_file_path`, `validate_string`,
//! `MAX_PLAN_FILE_SIZE`, etc.) are now `pub(crate)` in `plan_tool.rs`
//! so this file can reach them from outside the module.

use crate::brain::tools::plan_tool::{
    MAX_CONTEXT_LENGTH, MAX_DESCRIPTION_LENGTH, MAX_PLAN_FILE_SIZE, MAX_TITLE_LENGTH, PlanTool,
    default_complexity, validate_plan_file_path, validate_string,
};
use crate::brain::tools::{Tool, ToolExecutionContext};
use std::path::PathBuf;
use tempfile::TempDir;

// ── path validation ───────────────────────────────────────────────

#[test]
fn validate_path_within_working_directory() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path();

    let session_id = uuid::Uuid::new_v4();
    let plan_file = working_dir.join(format!(".opencrabs_plan_{}.json", session_id));

    let result = validate_plan_file_path(&plan_file, working_dir);
    assert!(result.is_ok());
}

#[test]
fn validate_path_outside_working_directory() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path();

    let session_id = uuid::Uuid::new_v4();
    // Try to write outside working directory
    let plan_file = PathBuf::from("/tmp").join(format!(".opencrabs_plan_{}.json", session_id));

    let result = validate_plan_file_path(&plan_file, working_dir);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("within the session directory")
    );
}

#[test]
fn validate_path_traversal_attack() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path();

    let session_id = uuid::Uuid::new_v4();
    // Try path traversal - construct a path that goes outside working_dir
    let parent = working_dir.parent().unwrap_or(working_dir);
    let plan_file = parent.join(format!(".opencrabs_plan_{}.json", session_id));

    let result = validate_plan_file_path(&plan_file, working_dir);
    assert!(result.is_err());
}

#[test]
fn validate_filename_pattern() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path();

    // Invalid filename (not matching pattern)
    let plan_file = working_dir.join("invalid_plan.json");

    let result = validate_plan_file_path(&plan_file, working_dir);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("must match pattern")
    );
}

#[test]
fn validate_filename_requires_uuid() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path();

    // Invalid UUID in filename
    let plan_file = working_dir.join(".opencrabs_plan_not-a-uuid.json");

    let result = validate_plan_file_path(&plan_file, working_dir);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("valid UUID"));
}

#[test]
#[cfg(unix)]
fn validate_symlink_rejection() {
    use std::os::unix::fs::symlink;

    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path();

    let session_id = uuid::Uuid::new_v4();
    let target_file = working_dir.join("target.json");
    let plan_file = working_dir.join(format!(".opencrabs_plan_{}.json", session_id));

    // Create a target file and symlink to it
    std::fs::write(&target_file, "{}").unwrap();
    symlink(&target_file, &plan_file).unwrap();

    let result = validate_plan_file_path(&plan_file, working_dir);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("symlink"));
}

// ── string validation ─────────────────────────────────────────────

#[test]
fn validate_string_empty() {
    let result = validate_string("", 100, "Test field");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("cannot be empty"));
}

#[test]
fn validate_string_whitespace_only() {
    let result = validate_string("   ", 100, "Test field");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("cannot be empty"));
}

#[test]
fn validate_string_exceeds_max_length() {
    let long_string = "a".repeat(300);
    let result = validate_string(&long_string, MAX_TITLE_LENGTH, "Title");
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("exceeds maximum length")
    );
}

#[test]
fn validate_string_valid() {
    let result = validate_string("Valid title", MAX_TITLE_LENGTH, "Title");
    assert!(result.is_ok());
}

#[test]
fn max_plan_file_size_constant() {
    // Verify the constant is reasonable (10MB)
    assert_eq!(MAX_PLAN_FILE_SIZE, 10 * 1024 * 1024);
}

#[test]
fn input_validation_limits() {
    // Verify limits are reasonable
    assert_eq!(MAX_TITLE_LENGTH, 200);
    assert_eq!(MAX_DESCRIPTION_LENGTH, 5000);
    assert_eq!(MAX_CONTEXT_LENGTH, 5000);
}

#[test]
fn default_complexity_is_three() {
    assert_eq!(default_complexity(), 3);
}

#[test]
fn validate_title_at_limit() {
    let title = "a".repeat(MAX_TITLE_LENGTH);
    let result = validate_string(&title, MAX_TITLE_LENGTH, "Title");
    assert!(result.is_ok());
}

#[test]
fn validate_title_one_over_limit() {
    let title = "a".repeat(MAX_TITLE_LENGTH + 1);
    let result = validate_string(&title, MAX_TITLE_LENGTH, "Title");
    assert!(result.is_err());
}

#[test]
fn validate_description_at_limit() {
    let desc = "a".repeat(MAX_DESCRIPTION_LENGTH);
    let result = validate_string(&desc, MAX_DESCRIPTION_LENGTH, "Description");
    assert!(result.is_ok());
}

#[test]
fn validate_context_at_limit() {
    let context = "a".repeat(MAX_CONTEXT_LENGTH);
    let result = validate_string(&context, MAX_CONTEXT_LENGTH, "Context");
    assert!(result.is_ok());
}

#[test]
fn filename_with_special_characters() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path();

    // Try filename with special characters that might be injection attempts
    let plan_file = working_dir.join(".opencrabs_plan_../../etc/passwd.json");

    let result = validate_plan_file_path(&plan_file, working_dir);
    assert!(result.is_err());
}

#[test]
fn filename_with_null_byte() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path();

    let session_id = uuid::Uuid::new_v4();
    let filename = format!(".opencrabs_plan_{}\0.json", session_id);
    let plan_file = working_dir.join(filename);

    // Rust's Path handling should prevent null bytes, but test anyway
    let result = validate_plan_file_path(&plan_file, working_dir);
    // Either fails validation or panic is caught
    assert!(result.is_err() || plan_file.to_str().is_none());
}

#[test]
fn validate_plan_file_path_canonical() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path();

    let session_id = uuid::Uuid::new_v4();
    // Use ./ which should resolve to working_dir
    let plan_file = working_dir.join(format!("./.opencrabs_plan_{}.json", session_id));

    // Should still validate correctly after canonicalization
    let result = validate_plan_file_path(&plan_file, working_dir);
    // May pass or fail depending on path resolution, but shouldn't panic
    let _ = result;
}

// ── import operation ──────────────────────────────────────────────
//
// PR #160 added the import operation alongside the sample plan
// fixture. The tests below cover the happy path plus the four
// error / hardening paths the original PR was missing: size cap,
// invalid JSON, orphan dependency UUIDs, and symlink rejection at
// the target file.

#[tokio::test]
async fn import_sample_plan_succeeds() {
    let json = include_str!("../brain/tools/test_data/sample-coding-plan.json");

    let tmp_dir = TempDir::new().unwrap();
    let plan_file = tmp_dir.path().join("sample-coding-plan.json");
    std::fs::write(&plan_file, json).unwrap();

    let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    let tool = PlanTool;

    let input = serde_json::json!({
        "operation": "import",
        "file_path": plan_file.to_str().unwrap(),
    });

    let result = tool.execute(input, &ctx).await.unwrap();
    assert!(result.success, "import must succeed on the sample plan");
    assert!(result.output.contains("Imported plan"));
    assert!(result.output.contains("7 tasks"));
}

#[tokio::test]
async fn import_rejects_file_over_size_cap() {
    // 10 MB + 1 byte triggers the size check before parse. This guards
    // against a malicious or runaway plan file blowing up memory on
    // read_to_string. The bytes don't need to be valid UTF-8 since the
    // size check fires before any parsing.
    let tmp_dir = TempDir::new().unwrap();
    let plan_file = tmp_dir.path().join("too_big.json");
    let payload = vec![b'a'; 10 * 1024 * 1024 + 1];
    std::fs::write(&plan_file, payload).unwrap();

    let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    let tool = PlanTool;
    let input = serde_json::json!({
        "operation": "import",
        "file_path": plan_file.to_str().unwrap(),
    });

    let err = tool
        .execute(input, &ctx)
        .await
        .expect_err("oversize import must error");
    let msg = err.to_string();
    assert!(
        msg.contains("too large"),
        "expected 'too large' size-cap error, got: {msg}"
    );
}

#[tokio::test]
async fn import_rejects_invalid_json() {
    let tmp_dir = TempDir::new().unwrap();
    let plan_file = tmp_dir.path().join("bad.json");
    std::fs::write(&plan_file, "{this is not valid json").unwrap();

    let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    let tool = PlanTool;
    let input = serde_json::json!({
        "operation": "import",
        "file_path": plan_file.to_str().unwrap(),
    });

    let err = tool
        .execute(input, &ctx)
        .await
        .expect_err("malformed JSON import must error");
    let msg = err.to_string();
    assert!(
        msg.contains("Invalid plan JSON"),
        "expected 'Invalid plan JSON' error, got: {msg}"
    );
}

#[tokio::test]
async fn import_rejects_orphan_dependency_uuid() {
    // A dependency that references a UUID not present in the imported
    // task set is a malformed plan. Silent `filter_map` dropping such
    // refs hid authoring mistakes; the import must reject with a
    // specific error so the user can fix the JSON.
    let bad_json = r#"{
        "id": "00000000-0000-0000-0000-000000000000",
        "session_id": "00000000-0000-0000-0000-000000000000",
        "title": "Bad Deps",
        "description": "Has a dep on a UUID not in the task list",
        "status": "Draft",
        "context": "",
        "risks": [],
        "test_strategy": "",
        "technical_stack": [],
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z",
        "approved_at": null,
        "tasks": [
            {
                "id": "11111111-1111-1111-1111-111111111111",
                "order": 1,
                "title": "Orphan dep task",
                "description": "Depends on a uuid that isn't here",
                "task_type": "Edit",
                "dependencies": ["99999999-9999-9999-9999-999999999999"],
                "complexity": 1,
                "acceptance_criteria": [],
                "status": "Pending",
                "notes": null,
                "completed_at": null
            }
        ]
    }"#;

    let tmp_dir = TempDir::new().unwrap();
    let plan_file = tmp_dir.path().join("orphan_dep.json");
    std::fs::write(&plan_file, bad_json).unwrap();

    let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    let tool = PlanTool;
    let input = serde_json::json!({
        "operation": "import",
        "file_path": plan_file.to_str().unwrap(),
    });

    let err = tool
        .execute(input, &ctx)
        .await
        .expect_err("orphan-dep import must error");
    let msg = err.to_string();
    assert!(
        msg.contains("depends on unknown task id"),
        "expected orphan-dep error, got: {msg}"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn import_rejects_symlink_at_target() {
    // The symlink check on the TARGET file (the import file itself)
    // still has to fire — a malicious user could place a symlink at
    // the import location pointing somewhere else and trick the agent
    // into reading from the resolved target. The PR's original
    // ancestor-walking approach was wrong (broke on macOS where /var
    // is a symlink), but the target-only check still has to catch a
    // symlink at the file itself.
    let tmp_dir = TempDir::new().unwrap();
    let real_file = tmp_dir.path().join("real.json");
    std::fs::write(&real_file, "{}").unwrap();
    let symlink_path = tmp_dir.path().join("link.json");
    std::os::unix::fs::symlink(&real_file, &symlink_path).unwrap();

    let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    let tool = PlanTool;
    let input = serde_json::json!({
        "operation": "import",
        "file_path": symlink_path.to_str().unwrap(),
    });

    let err = tool
        .execute(input, &ctx)
        .await
        .expect_err("symlink target import must error");
    let msg = err.to_string();
    assert!(
        msg.contains("symlink"),
        "expected symlink rejection, got: {msg}"
    );
}
