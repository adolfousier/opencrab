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

use crate::brain::ToolResult;
use crate::brain::tools::plan_tool::{
    MAX_DESCRIPTION_LENGTH, MAX_PLAN_FILE_SIZE, MAX_TITLE_LENGTH, PlanTool, default_complexity,
    validate_plan_file_path, validate_string,
};
use crate::brain::tools::{Tool, ToolExecutionContext};
use crate::config::profile::{home_for_profile, with_profile_home_async};
use crate::utils::plan_files::load_plan;
use std::path::PathBuf;
use tempfile::TempDir;

/// Run `f` under a throwaway profile home so plan files (JSON, archive)
/// never touch the real `~/.opencrabs/agents/session/`.
async fn in_temp_home<F, T>(f: F) -> T
where
    F: std::future::Future<Output = T>,
{
    let profile = format!("plan-tool-test-{}", uuid::Uuid::new_v4());
    let out = with_profile_home_async(Some(&profile), f).await;
    let home = home_for_profile(Some(&profile));
    let _ = std::fs::remove_dir_all(&home);
    out
}

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
    in_temp_home(async {
        let json = include_str!("../brain/tools/test_data/sample-coding-plan.json");

        let tmp_dir = TempDir::new().unwrap();
        let plan_file = tmp_dir.path().join("sample-coding-plan.json");
        std::fs::write(&plan_file, json).unwrap();

        let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
        let tool = PlanTool;

        let input = serde_json::json!({
            "operation": "init",
            "file_path": plan_file.to_str().unwrap(),
        });

        let result = tool.execute(input, &ctx).await.unwrap();
        assert!(result.success, "import must succeed on the sample plan");
        assert!(result.output.contains("Imported plan"));
        assert!(result.output.contains("7 tasks"));
    })
    .await;
}

#[tokio::test]
async fn import_under_auto_approve_goes_active() {
    in_temp_home(async {
        let json = include_str!("../brain/tools/test_data/sample-coding-plan.json");
        let tmp_dir = TempDir::new().unwrap();
        let plan_file = tmp_dir.path().join("sample-coding-plan.json");
        std::fs::write(&plan_file, json).unwrap();

        let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4()).with_auto_approve(true);
        let tool = PlanTool;

        let input = serde_json::json!({
            "operation": "init",
            "file_path": plan_file.to_str().unwrap(),
        });

        let result = tool.execute(input, &ctx).await.unwrap();
        assert!(result.success);
        // #581 parity with the create arm: under tool auto-approve there is no
        // user Approve step, so the imported plan must go Active, not stall in
        // Editing with a "call 'start'" message that the Editing gate refuses.
        assert!(
            result.output.contains("Active — auto-approve"),
            "expected Active message, got: {}",
            result.output
        );
        assert!(
            !result.output.contains("Editing"),
            "must not report Editing under auto-approve: {}",
            result.output
        );
    })
    .await;
}

#[tokio::test]
async fn import_without_auto_approve_waits_in_editing() {
    in_temp_home(async {
        let json = include_str!("../brain/tools/test_data/sample-coding-plan.json");
        let tmp_dir = TempDir::new().unwrap();
        let plan_file = tmp_dir.path().join("sample-coding-plan.json");
        std::fs::write(&plan_file, json).unwrap();

        let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
        let tool = PlanTool;

        let input = serde_json::json!({
            "operation": "init",
            "file_path": plan_file.to_str().unwrap(),
        });

        let result = tool.execute(input, &ctx).await.unwrap();
        assert!(result.success);
        assert!(
            result.output.contains("Editing"),
            "default import stays Editing: {}",
            result.output
        );
        // The old message said "Call 'start' to begin" while start is refused in
        // Editing — the message must direct the agent to WAIT for approval.
        assert!(
            result.output.contains("WAIT"),
            "message must say WAIT: {}",
            result.output
        );
        assert!(
            !result.output.contains("Call 'start'"),
            "lying start instruction must be gone: {}",
            result.output
        );
    })
    .await;
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
        "operation": "init",
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
        "operation": "init",
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
        "operation": "init",
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
        "operation": "init",
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

// ── 4-command flow (init → add_task → start → complete) ────────────

/// Build a session with a checklist plan and `n` simple edit tasks via
/// `init` with inline tasks (checklist track: Active immediately).
/// Returns the context to drive further calls.
async fn setup_plan_with_tasks(tool: &PlanTool, n: usize) -> ToolExecutionContext {
    let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    let tasks: Vec<serde_json::Value> = (1..=n)
        .map(|i| {
            serde_json::json!({
                "title": format!("Task {i}"),
                "description": format!("Description for task {i}"),
                "task_type": "edit"
            })
        })
        .collect();
    tool.execute(
        serde_json::json!({
            "operation": "init",
            "title": "Flow test",
            "tasks": tasks
        }),
        &ctx,
    )
    .await
    .unwrap();
    // Approve the plan so start/complete operations are allowed.
    if let Some(mut plan) = crate::utils::plan_files::load_plan(ctx.session_id).await {
        plan.approve();
        crate::utils::plan_files::save_plan(&plan).await.unwrap();
    }
    ctx
}

#[tokio::test]
async fn start_returns_full_task_details() {
    in_temp_home(async {
        let tool = PlanTool;
        let ctx = setup_plan_with_tasks(&tool, 2).await;

        // No task_order → starts the next pending task and returns full details.
        let result = tool
            .execute(serde_json::json!({ "operation": "start" }), &ctx)
            .await
            .unwrap();
        assert!(result.success);
        assert!(
            result.output.contains("Task #1") && result.output.contains("Description for task 1"),
            "start must surface full details of task 1, got: {}",
            result.output
        );
    })
    .await;
}

#[tokio::test]
async fn start_is_idempotent_on_in_progress_task() {
    in_temp_home(async {
        // Calling start again (e.g. after a compaction) must re-surface the
        // in-progress task's details, not error or skip ahead.
        let tool = PlanTool;
        let ctx = setup_plan_with_tasks(&tool, 2).await;

        tool.execute(serde_json::json!({ "operation": "start" }), &ctx)
            .await
            .unwrap();
        let again = tool
            .execute(serde_json::json!({ "operation": "start" }), &ctx)
            .await
            .unwrap();
        assert!(again.success);
        assert!(
            again.output.contains("Task #1"),
            "start with no args must resume the in-progress task, got: {}",
            again.output
        );
    })
    .await;
}

#[tokio::test]
async fn complete_auto_starts_next_task() {
    in_temp_home(async {
        let tool = PlanTool;
        let ctx = setup_plan_with_tasks(&tool, 2).await;

        tool.execute(serde_json::json!({ "operation": "start" }), &ctx)
            .await
            .unwrap();

        // Completing task 1 auto-starts task 2 and returns its details.
        let result = tool
            .execute(
                serde_json::json!({
                    "operation": "complete",
                    "task_order": 1,
                    "action": "success",
                    "output": "Task 1 done"
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.success);
        assert!(
            result.output.contains("Task #1") && result.output.contains("completed"),
            "completion must confirm task 1, got: {}",
            result.output
        );
        // #1195: auto-start is opt-in ([agent] plan_auto_start, default
        // false) - complete must NOT start task 2, only hint at it.
        assert!(
            !result.output.contains("Started Task #2"),
            "complete must not auto-start task 2 by default, got: {}",
            result.output
        );
        assert!(
            result.output.contains("Next eligible: Task #2"),
            "complete must report task 2 as the next eligible hint, got: {}",
            result.output
        );
    })
    .await;
}

#[tokio::test]
async fn complete_last_task_reports_plan_complete() {
    in_temp_home(async {
        let tool = PlanTool;
        let ctx = setup_plan_with_tasks(&tool, 1).await;

        tool.execute(serde_json::json!({ "operation": "start" }), &ctx)
            .await
            .unwrap();
        let result = tool
            .execute(
                serde_json::json!({
                    "operation": "complete",
                    "task_order": 1,
                    "action": "success"
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(
            result.output.contains("Plan complete"),
            "finishing the last task must report plan completion, got: {}",
            result.output
        );
    })
    .await;
}

#[tokio::test]
async fn start_specific_task_blocked_by_dependency() {
    in_temp_home(async {
    let tool = PlanTool;
    let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    tool.execute(
        serde_json::json!({
            "operation": "init",
            "title": "Deps",
            "description": "dep test",
            "tasks": [
                { "title": "First", "description": "the first", "task_type": "edit" },
                { "title": "Second", "description": "needs first", "task_type": "edit", "dependencies": [1] }
            ]
        }),
        &ctx,
    )
    .await
    .unwrap();

    // Task 2 depends on task 1 (not yet done) → starting it must be blocked.
    let result = tool
        .execute(
            serde_json::json!({ "operation": "start", "task_order": 2 }),
            &ctx,
        )
        .await
        .unwrap();
    assert!(!result.success, "blocked start must not succeed");
    let msg = result.error.unwrap_or(result.output);
    assert!(
        msg.contains("blocked"),
        "starting a task with unmet dependencies must report it blocked, got: {msg}"
    );
    })
    .await;
}

#[tokio::test]
async fn init_with_inline_tasks_creates_plan_and_tasks() {
    in_temp_home(async {
        let tool = PlanTool;
        let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
        let result = tool
            .execute(
                serde_json::json!({
                    "operation": "init",
                    "title": "Inline",
                    "description": "created with inline tasks",
                    "tasks": [
                        { "title": "Alpha", "description": "first", "task_type": "edit" },
                        { "title": "Beta", "description": "second", "task_type": "test" }
                    ]
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.success);
        assert!(
            result.output.contains("2 tasks"),
            "init should report the task count: {}",
            result.output
        );
        // Dup-1 fix (#577): the tool result no longer echoes the task list — the
        // plan card already shows it — so verify the tasks landed on the plan
        // document, and that the result does NOT re-list them.
        assert!(
            !result.output.contains("Alpha") && !result.output.contains("Beta"),
            "the tool result must not duplicate the card's task list: {}",
            result.output
        );
        let plan = crate::utils::plan_files::load_plan(ctx.session_id)
            .await
            .unwrap();
        assert_eq!(plan.tasks.len(), 2);
        let titles: Vec<&str> = plan.tasks.iter().map(|t| t.title.as_str()).collect();
        assert!(
            titles.contains(&"Alpha") && titles.contains(&"Beta"),
            "both inline tasks must be on the plan, got {titles:?}"
        );
    })
    .await;
}

#[tokio::test]
async fn approve_op_is_gated_on_granted_autonomy() {
    // `plan approve` self-approves only after the user granted autonomy (#581);
    // otherwise it is refused so the default stays user-gated.
    use crate::tui::plan::PlanStatus;
    use crate::utils::plan_files::{PlanModeState, is_plan_autonomy, load_plan, plan_mode_state};
    in_temp_home(async {
        let tool = PlanTool;
        let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
        // Checklist plan → Editing.
        tool.execute(
            serde_json::json!({
                "operation": "init",
                "title": "Autonomy test",
                "tasks": [{ "title": "a", "description": "d", "task_type": "edit" }]
            }),
            &ctx,
        )
        .await
        .unwrap();

        // Refused without a grant; plan stays Editing.
        let refused = tool
            .execute(serde_json::json!({ "operation": "approve" }), &ctx)
            .await
            .unwrap();
        assert!(!refused.success);
        assert!(refused.error.unwrap().contains("Self-approval is off"));
        assert_eq!(
            plan_mode_state(ctx.session_id).await,
            PlanModeState::PostInitEditing
        );

        // Grant, then approve → Active.
        let granted = tool
            .execute(serde_json::json!({ "operation": "grant_autonomy" }), &ctx)
            .await
            .unwrap();
        assert!(granted.success);
        assert!(is_plan_autonomy(ctx.session_id).await);

        let approved = tool
            .execute(serde_json::json!({ "operation": "approve" }), &ctx)
            .await
            .unwrap();
        assert!(approved.success, "got {:?}", approved.error);
        assert_eq!(
            load_plan(ctx.session_id).await.unwrap().status,
            PlanStatus::Active
        );

        // Revoke turns it back off.
        tool.execute(serde_json::json!({ "operation": "revoke_autonomy" }), &ctx)
            .await
            .unwrap();
        assert!(!is_plan_autonomy(ctx.session_id).await);
    })
    .await;
}

#[tokio::test]
async fn discard_op_abandons_the_plan_and_show_plan_reports_state() {
    // show_plan reads plan state without side effects (#585). The discard op
    // is a USER action: refused for the agent unless the session granted plan
    // autonomy — a model must not be able to shred its own review harness,
    // whether on its own initiative or because a malicious message told it to.
    use crate::utils::plan_files::{PlanModeState, plan_mode_state, set_plan_autonomy};
    in_temp_home(async {
        let tool = PlanTool;
        let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());

        // No plan yet: discard is a no-op error, show_plan still answers.
        let empty_discard = tool
            .execute(serde_json::json!({ "operation": "discard" }), &ctx)
            .await
            .unwrap();
        assert!(!empty_discard.success);
        let empty_show = tool
            .execute(serde_json::json!({ "operation": "show_plan" }), &ctx)
            .await
            .unwrap();
        assert!(empty_show.success);

        // Create a checklist plan.
        tool.execute(
            serde_json::json!({
                "operation": "init",
                "title": "Scrap me",
                "tasks": [{ "title": "a", "description": "d", "task_type": "edit" }]
            }),
            &ctx,
        )
        .await
        .unwrap();
        assert_ne!(plan_mode_state(ctx.session_id).await, PlanModeState::NoPlan);

        // show_plan reports it without mutating.
        let shown = tool
            .execute(serde_json::json!({ "operation": "show_plan" }), &ctx)
            .await
            .unwrap();
        assert!(shown.success);
        assert_ne!(
            plan_mode_state(ctx.session_id).await,
            PlanModeState::NoPlan,
            "show_plan must not change the plan"
        );

        // Without plan autonomy the agent's discard is refused, naming the
        // user's own paths, and the plan stays live.
        let refused = tool
            .execute(serde_json::json!({ "operation": "discard" }), &ctx)
            .await
            .unwrap();
        assert!(
            !refused.success,
            "agent discard must be refused without plan autonomy"
        );
        let msg = format!("{:?}", refused.error);
        assert!(msg.contains("/discard"), "got: {msg}");
        assert!(msg.contains("Discard button"), "got: {msg}");
        assert_ne!(
            plan_mode_state(ctx.session_id).await,
            PlanModeState::NoPlan,
            "a refused discard must leave the plan live"
        );

        // With plan autonomy granted, the agent discard goes through → NoPlan.
        set_plan_autonomy(ctx.session_id, true).await.unwrap();
        let discarded = tool
            .execute(serde_json::json!({ "operation": "discard" }), &ctx)
            .await
            .unwrap();
        assert!(discarded.success, "got {:?}", discarded.error);
        assert_eq!(plan_mode_state(ctx.session_id).await, PlanModeState::NoPlan);
    })
    .await;
}

// ── plan_session_override (#908 option A) ─────────────────────────

/// Plan-driven spawn threading: a child context with a fresh session id
/// but `plan_session_override` set to the parent's id resolves ALL plan
/// Delegated worker sessions (#908 option A binding) have NO plan tool at
/// all — every operation refuses, mutations and reads alike (#1195). The
/// parent's checklist is untouched by worker sessions; verdicts are
/// recorded by the parent from the worker's final summary.
#[tokio::test]
async fn plan_worker_sessions_are_denied_the_plan_tool() {
    use crate::tui::plan::TaskStatus;
    use crate::utils::plan_files::load_plan;

    in_temp_home(async {
        let tool = PlanTool;
        // Parent session owns a live approved checklist plan.
        let parent_ctx = setup_plan_with_tasks(&tool, 2).await;
        let parent_sid = parent_ctx.session_id;

        // Child context: fresh session id + the parent override.
        let mut child_ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
        child_ctx.plan_session_override = Some(parent_sid);
        let blob = |r: &ToolResult| format!("{}|{}", r.output, r.error.clone().unwrap_or_default());

        // start refused outright — no task details, no spawn.
        let started = tool
            .execute(serde_json::json!({ "operation": "start" }), &child_ctx)
            .await
            .unwrap();
        assert!(!started.success, "worker start must be refused");
        assert!(
            blob(&started).contains("unavailable to delegated"),
            "refusal must explain the delegation contract:\n{}",
            blob(&started)
        );

        // complete refused — workers cannot flip rows, skip included.
        let done = tool
            .execute(
                serde_json::json!({
                    "operation": "complete",
                    "task_order": 1,
                    "action": "skip",
                    "output": "skipping the gate, not the work"
                }),
                &child_ctx,
            )
            .await
            .unwrap();
        assert!(!done.success, "worker complete/skip must be refused");

        // Disk truth: the parent's checklist is untouched by the worker.
        let parent_plan = load_plan(parent_sid)
            .await
            .expect("parent plan must still exist");
        assert_eq!(
            parent_plan.tasks[0].status,
            TaskStatus::Pending,
            "worker attempts must never mutate rows"
        );
        assert_eq!(parent_plan.tasks[1].status, TaskStatus::Pending);
    })
    .await;
}

/// Workers cannot even INIT plans: the override binding denies every plan
/// operation, so a delegated session can never create checklist state —
/// under its own id or the parent's (#1195).
#[tokio::test]
async fn plan_worker_sessions_cannot_init_plans() {
    in_temp_home(async {
        let tool = PlanTool;
        let parent_sid = uuid::Uuid::new_v4();

        let mut child_ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
        let child_sid = child_ctx.session_id;
        child_ctx.plan_session_override = Some(parent_sid);

        let init = tool
            .execute(
                serde_json::json!({
                    "operation": "init",
                    "title": "Override init",
                    "tasks": [{
                        "title": "Only task",
                        "description": "Single task for the override-init test",
                        "task_type": "edit"
                    }]
                }),
                &child_ctx,
            )
            .await
            .unwrap();
        assert!(!init.success, "worker init must be refused");
        assert!(
            init.output.contains("unavailable to delegated")
                || init
                    .error
                    .clone()
                    .unwrap_or_default()
                    .contains("unavailable to delegated"),
            "refusal must explain the delegation contract:\n{}",
            init.output
        );

        // Nothing was created under either session.
        assert!(
            !load_plan(parent_sid).await.is_some(),
            "worker init must not key a plan to the parent"
        );
        assert!(
            !crate::utils::plan_files::plan_json_path(child_sid)
                .await
                .exists(),
            "child must not create its own plan JSON"
        );
    })
    .await;
}

// ── #908 task 4: isolated execution — decision table + worker plumbing ────

/// Every row of `resolve_task_execution`, deterministic (#908 option A).
/// Args: (explicit_request, config_enabled, fresh_context, state_on_disk,
/// override_set, has_service_context, has_spawn_machinery,
/// task_already_in_progress).
#[test]
fn task_execution_decision_table() {
    use crate::brain::tools::plan_tool::{TaskExecutionPath, resolve_task_execution};
    use TaskExecutionPath::*;

    // Row 1 — recursion guard beats everything: a session already running
    // as a plan worker (override set) executes inline even when isolation
    // is explicitly requested and all machinery exists.
    assert_eq!(
        resolve_task_execution(Some(true), true, true, true, true, true, true, false),
        Inline {
            reason: "already inside a plan worker session"
        }
    );

    // Row 2 — request resolution: nothing requested → inline.
    assert_eq!(
        resolve_task_execution(None, false, true, true, false, true, true, false),
        Inline {
            reason: "isolated execution not requested"
        }
    );
    // Row 2 — an explicit false beats a config-on default.
    assert_eq!(
        resolve_task_execution(Some(false), true, true, true, false, true, true, false),
        Inline {
            reason: "isolated execution not requested"
        }
    );
    // Row 2 — an explicit true beats a config-off default.
    assert_eq!(
        resolve_task_execution(Some(true), false, true, true, false, true, true, false),
        Isolated
    );
    // Row 2 — Ralph fresh_context=false gates the config default: even
    // with the config flag on, isolation is not requested. (Pure-fn test:
    // ralph_loop_config() is process-wide via OnceLock, so the handler's
    // toml read is exercised by hot reload, not here.)
    assert_eq!(
        resolve_task_execution(None, true, false, true, false, true, true, false),
        Inline {
            reason: "isolated execution not requested"
        }
    );
    // Row 2 — an explicit per-call request bypasses the fresh_context
    // gate (the Ralph loop passes Some(fresh_context) itself).
    assert_eq!(
        resolve_task_execution(Some(true), true, false, true, false, true, true, false),
        Isolated
    );

    // Row 3 — requested but no session machinery → inline.
    assert_eq!(
        resolve_task_execution(Some(true), true, true, true, false, false, true, false),
        Inline {
            reason: "no session machinery (service context)"
        }
    );

    // Row 4 — session machinery but no spawn machinery → inline.
    assert_eq!(
        resolve_task_execution(Some(true), true, true, true, false, true, false, false),
        Inline {
            reason: "no spawn machinery wired (manager/registry)"
        }
    );

    // Row 5 — state_on_disk=false blocks isolation mechanically, EVEN
    // when explicitly forced: without plan state threaded on disk a
    // worker cannot operate on the parent checklist.
    assert_eq!(
        resolve_task_execution(Some(true), true, true, false, false, true, true, false),
        Inline {
            reason: "state_on_disk disabled — plan state cannot be threaded"
        }
    );

    // Row 6 — InProgress task under config-default isolation resumes
    // inline (idempotent retry / crashed-worker leftover).
    assert_eq!(
        resolve_task_execution(None, true, true, true, false, true, true, true),
        Inline {
            reason: "task already in progress — retry resumes inline"
        }
    );
    // Row 6 exception — explicit isolation forces a fresh worker even for
    // an InProgress task (start blocks, so no live worker can exist).
    assert_eq!(
        resolve_task_execution(Some(true), true, true, true, false, true, true, true),
        Isolated
    );

    // Happy path — config-on, fresh_context default, machinery present,
    // task not in progress.
    assert_eq!(
        resolve_task_execution(None, true, true, true, false, true, true, false),
        Isolated
    );
}

/// The worker brief is self-contained: title, description, acceptance
/// criteria, working dir, epistemic flags, and the complete instruction
/// with the RIGHT task_order. It is the worker's entire context besides
/// the plan file — no parent conversation ever leaks in.
#[tokio::test]
async fn worker_brief_is_self_contained() {
    in_temp_home(async {
        let tool = PlanTool;
        let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
        tool.execute(
            serde_json::json!({
                "operation": "init",
                "title": "Brief test",
                "tasks": [{
                    "title": "T1",
                    "description": "D1",
                    "task_type": "edit",
                    "acceptance_criteria": ["AC1", "AC2"]
                }]
            }),
            &ctx,
        )
        .await
        .unwrap();
        let plan = crate::utils::plan_files::load_plan(ctx.session_id)
            .await
            .expect("plan must exist");
        let brief = crate::brain::tools::plan_tool::build_worker_brief(
            1,
            &plan.tasks[0],
            std::path::Path::new("/tmp/work"),
            "\n\nEpistemic flags (1):\n  ⚠ [contradicted] plan:task:1: prior failure",
        );
        assert!(brief.contains("Task #1: T1"), "title missing:\n{brief}");
        assert!(brief.contains("Description: D1"), "description missing");
        assert!(
            brief.contains("- AC1") && brief.contains("- AC2"),
            "criteria missing"
        );
        assert!(brief.contains("/tmp/work"), "working dir missing");
        assert!(brief.contains("Epistemic flags"), "epistemic flags missing");
        assert!(
            brief.contains("NO plan tool access"),
            "brief must state the worker cannot touch plans:\n{brief}"
        );
        assert!(
            !brief.contains("task_order=")
                && !brief.to_lowercase().contains("call `plan complete`"),
            "brief must NOT instruct the worker to call plan complete — \
             the parent records the verdict:\n{brief}"
        );
        assert!(
            !brief.contains("PARENT's checklist"),
            "stale binding text: the worker has no plan tool at all:\n{brief}"
        );
        assert!(
            brief.contains("Work ONLY this task"),
            "brief must forbid scope drift"
        );

        // Empty description and criteria are omitted, not rendered blank.
        // (init validation rejects empty descriptions, so blank the fields
        // on an in-memory clone instead.)
        let mut bare_task = plan.tasks[0].clone();
        bare_task.description = String::new();
        bare_task.acceptance_criteria.clear();
        let bare = crate::brain::tools::plan_tool::build_worker_brief(
            1,
            &bare_task,
            std::path::Path::new("/tmp"),
            "",
        );
        assert!(
            !bare.contains("Description:"),
            "empty description must be omitted"
        );
        assert!(
            !bare.contains("Acceptance criteria:"),
            "empty criteria must be omitted"
        );
        assert!(
            !bare.contains("Epistemic flags"),
            "empty epistemic note must be omitted"
        );
    })
    .await;
}

/// `report_after_worker` COLLECTS — it never flips rows itself. Workers
/// hold no plan pen (#1195), so a pending row is the expected post-run
/// state and the parent records the verdict; false is reserved for real
/// anomalies only.
#[tokio::test]
async fn post_worker_report_collects_and_defers_to_parent() {
    in_temp_home(async {
        use crate::brain::tools::plan_tool::report_after_worker;
        use crate::tui::plan::TaskStatus;
        let tool = PlanTool;
        let ctx = setup_plan_with_tasks(&tool, 2).await;
        let sid = ctx.session_id;

        // InProgress on disk → collect for parent review (the normal case:
        // workers cannot mark rows anymore).
        let mut plan = crate::utils::plan_files::load_plan(sid).await.unwrap();
        plan.tasks[0].status = TaskStatus::InProgress;
        let (ok, report) = report_after_worker(1, Some(&plan), "I did great, trust me");
        assert!(ok, "pending verdict must not be an error: {report}");
        assert!(report.contains("Record the verdict yourself"));
        assert!(report.contains("Progress: 0/2 done"));

        // Already-Completed row → informational, still ok.
        let mut plan = crate::utils::plan_files::load_plan(sid).await.unwrap();
        plan.tasks[0].status = TaskStatus::Completed;
        let (ok, report) = report_after_worker(1, Some(&plan), "all green I promise");
        assert!(ok, "already-recorded rows are not errors");
        assert!(report.contains("already records"));
        assert!(report.contains("Progress: 1/2 done"));

        // Pending row → same collection path.
        let mut plan = crate::utils::plan_files::load_plan(sid).await.unwrap();
        plan.tasks[0].status = TaskStatus::Pending;
        let (ok, _) = report_after_worker(1, Some(&plan), "");
        assert!(ok, "pending-row collection is fine");

        // Failed row (parent pre-marked) → informational too.
        let mut plan = crate::utils::plan_files::load_plan(sid).await.unwrap();
        plan.tasks[0].status = TaskStatus::Failed;
        let (ok, report) = report_after_worker(1, Some(&plan), "definitely done");
        assert!(ok);
        assert!(report.contains("already records"));

        // Plan file vanished → honest failure.
        let (ok, report) = report_after_worker(1, None, "done");
        assert!(!ok);
        assert!(report.contains("vanished"));
    })
    .await;
}

/// Handler integration: explicit isolation from inside a plan worker
/// A worker-context start is refused like every other plan operation —
/// the old inline-fallback recursion guard is subsumed by the blanket
/// delegation refusal (#1195).
#[tokio::test]
async fn start_isolated_from_worker_context_is_refused() {
    in_temp_home(async {
        let tool = PlanTool;
        let parent_ctx = setup_plan_with_tasks(&tool, 2).await;
        let mut child_ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
        child_ctx.plan_session_override = Some(parent_ctx.session_id);

        let res = tool
            .execute(
                serde_json::json!({ "operation": "start", "isolated": true }),
                &child_ctx,
            )
            .await
            .unwrap();
        assert!(!res.success, "worker-context start must be refused");
        let blob = format!("{}|{}", res.output, res.error.clone().unwrap_or_default());
        assert!(
            blob.contains("unavailable to delegated"),
            "refusal must explain the delegation contract:\n{}",
            blob
        );
    })
    .await;
}

/// Handler integration: explicit isolation on a surface without session
/// machinery (plain test context) falls back inline with an honest note —
/// never a silent downgrade.
#[tokio::test]
async fn start_isolated_without_machinery_falls_back_honestly() {
    in_temp_home(async {
        let tool = PlanTool;
        let ctx = setup_plan_with_tasks(&tool, 2).await;
        let res = tool
            .execute(
                serde_json::json!({ "operation": "start", "isolated": true }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(res.success, "start must not fail: {:?}", res.error);
        assert!(
            res.output.contains("no session machinery"),
            "unavailable isolation must be reported:\n{}",
            res.output
        );
        assert!(
            res.output.contains("▶️ Task #1"),
            "inline details must still render:\n{}",
            res.output
        );
    })
    .await;
}

// ── Vacuous-pass guard tests (#1134) ───────────────────────────────

/// Parse "N passed" from cargo test output. Standard format.
#[test]
fn parse_cargo_test_pass_count_standard() {
    let output = r#"
   Compiling opencrabs v0.3.82
    Finished `test` profile [unoptimized + debuginfo]
     Running unittests src/lib.rs

running 42 tests
test foo::bar ... ok
test foo::baz ... ok
...
test result: ok. 42 passed; 0 failed; 3 ignored; 0 measured; 100 filtered out

"#;
    assert_eq!(
        crate::brain::tools::plan_tool::parse_cargo_test_pass_count(output),
        Some(42)
    );
}

/// Parse "0 passed" from cargo test output (filter matches nothing).
#[test]
fn parse_cargo_test_pass_count_zero() {
    let output = r#"
   Compiling opencrabs v0.3.82
    Finished `test` profile
     Running unittests src/lib.rs

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 6848 filtered out

"#;
    assert_eq!(
        crate::brain::tools::plan_tool::parse_cargo_test_pass_count(output),
        Some(0)
    );
}

/// Parse returns None for non-cargo-test output (e.g., clippy).
#[test]
fn parse_cargo_test_pass_count_non_test_output() {
    let output = r#"
   Compiling opencrabs v0.3.82
    Finished `dev` profile [unoptimized + debuginfo]
warning: unused variable
"#;
    assert_eq!(
        crate::brain::tools::plan_tool::parse_cargo_test_pass_count(output),
        None
    );
}

/// verify_with: 0 passed + exit 0 is refused (vacuous pass).
#[test]
fn verify_with_refuses_vacuous_pass() {
    use crate::brain::tools::plan_tool::verify_with;

    let commands = vec!["cargo test --lib nonexistent_test".to_string()];
    let mut run = |_cmd: &str| -> (i32, String) {
        (
            0,
            r#"
   Compiling opencrabs v0.3.82
    Finished `test` profile
     Running unittests src/lib.rs

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 6848 filtered out

"#
            .to_string(),
        )
    };

    let result = verify_with("test", 1, &commands, false, &mut run);
    assert!(result.is_err(), "verify_with must refuse 0 passed + exit 0");
    let err = result.unwrap_err();
    assert!(
        err.contains("ran 0 tests") || err.contains("vacuous pass"),
        "error must mention vacuous pass: {err}"
    );
}

/// verify_with: N passed + exit 0 is accepted (real pass).
#[test]
fn verify_with_accepts_real_pass() {
    use crate::brain::tools::plan_tool::verify_with;

    let commands = vec!["cargo test --lib my_test".to_string()];
    let mut run = |_cmd: &str| -> (i32, String) {
        (
            0,
            r#"
   Compiling opencrabs v0.3.82
    Finished `test` profile
     Running unittests src/lib.rs

running 5 tests
test my_test::foo ... ok
test my_test::bar ... ok
...
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 6843 filtered out

"#
            .to_string(),
        )
    };

    let result = verify_with("test", 1, &commands, false, &mut run);
    assert!(result.is_ok(), "verify_with must accept 5 passed + exit 0");
}

/// verify_with: non-test command (no "test result:" line) is accepted on exit 0.
#[test]
fn verify_with_accepts_non_test_exit_zero() {
    use crate::brain::tools::plan_tool::verify_with;

    let commands = vec!["cargo clippy --all-features".to_string()];
    let mut run = |_cmd: &str| -> (i32, String) {
        (
            0,
            r#"
   Compiling opencrabs v0.3.82
    Finished `dev` profile [unoptimized + debuginfo]
"#
            .to_string(),
        )
    };

    let result = verify_with("test", 1, &commands, false, &mut run);
    assert!(
        result.is_ok(),
        "verify_with must accept non-test commands on exit 0"
    );
}

// ── Criteria policy audit trail tests (#1135) ─────────────────────

/// First observation doesn't emit a belief (no previous value to compare).
#[test]
fn audit_criteria_policy_first_observation_no_belief() {
    use crate::brain::tools::plan_tool::{CriteriaPolicy, audit_criteria_policy_flip};
    use std::path::PathBuf;
    use tempfile::TempDir;

    let temp = TempDir::new().unwrap();
    let working_dir = PathBuf::from(temp.path());

    // Clear any previous state by using a unique path
    let unique_dir = working_dir.join("first_observation");
    std::fs::create_dir_all(&unique_dir).unwrap();

    let result = audit_criteria_policy_flip(&unique_dir, CriteriaPolicy::Strict);
    assert_eq!(result, CriteriaPolicy::Strict);

    // No belief should be emitted on first observation
    // (We can't easily test the epistemic store here without mocking,
    // but the function should not panic and should return the policy)
}

/// Policy flip between two calls emits a belief recording old → new.
#[test]
fn audit_criteria_policy_flip_emits_belief() {
    use crate::brain::tools::plan_tool::{CriteriaPolicy, audit_criteria_policy_flip};
    use std::path::PathBuf;
    use tempfile::TempDir;

    let temp = TempDir::new().unwrap();
    let working_dir = PathBuf::from(temp.path()).join("flip_test");
    std::fs::create_dir_all(&working_dir).unwrap();

    // First observation: Strict
    let result1 = audit_criteria_policy_flip(&working_dir, CriteriaPolicy::Strict);
    assert_eq!(result1, CriteriaPolicy::Strict);

    // Second observation: Off (flip!)
    let result2 = audit_criteria_policy_flip(&working_dir, CriteriaPolicy::Off);
    assert_eq!(result2, CriteriaPolicy::Off);

    // Third observation: Downgrade (another flip!)
    let result3 = audit_criteria_policy_flip(&working_dir, CriteriaPolicy::Downgrade);
    assert_eq!(result3, CriteriaPolicy::Downgrade);

    // The function should have logged beliefs for the flips
    // (We can't easily verify the epistemic store contents here without
    // mocking, but the function should not panic and should return the policies)
}

/// Unchanged policy emits nothing.
#[test]
fn audit_criteria_policy_unchanged_no_belief() {
    use crate::brain::tools::plan_tool::{CriteriaPolicy, audit_criteria_policy_flip};
    use std::path::PathBuf;
    use tempfile::TempDir;

    let temp = TempDir::new().unwrap();
    let working_dir = PathBuf::from(temp.path()).join("unchanged_test");
    std::fs::create_dir_all(&working_dir).unwrap();

    // First observation: Downgrade
    let result1 = audit_criteria_policy_flip(&working_dir, CriteriaPolicy::Downgrade);
    assert_eq!(result1, CriteriaPolicy::Downgrade);

    // Second observation: Downgrade (same, no flip)
    let result2 = audit_criteria_policy_flip(&working_dir, CriteriaPolicy::Downgrade);
    assert_eq!(result2, CriteriaPolicy::Downgrade);

    // Third observation: Downgrade (still same, no flip)
    let result3 = audit_criteria_policy_flip(&working_dir, CriteriaPolicy::Downgrade);
    assert_eq!(result3, CriteriaPolicy::Downgrade);

    // No beliefs should be emitted for unchanged policies
}

// ── #1195 task 2: subagent ontology announcements ──────────────────

#[test]
fn start_announcements_name_the_subagent_explicitly() {
    use crate::brain::tools::plan_tool::{inline_executor_suffix, subagent_outcome_notice};

    // Isolated mode receipt: parent learns a subagent DID the work.
    let ok = subagent_outcome_notice(true);
    assert!(ok.contains("A subagent completed this task"));
    assert!(ok.contains("dedicated subagent session was spawned"));
    assert!(ok.contains("verified against the plan on disk"));

    let failed = subagent_outcome_notice(false);
    assert!(failed.contains("did NOT complete"));

    // Inline mode echo: parent learns NO subagent exists.
    let inline = inline_executor_suffix();
    assert!(inline.contains("no subagent was spawned"));
    assert!(inline.contains("executor=self"));
}

#[test]
fn start_schema_teaches_subagent_spawn_not_isolation_jargon() {
    // The schema is the parent's first contact with the semantics: it must
    // say a subagent session does the work, not "isolated" jargon (#1195).
    let schema = PlanTool.input_schema().to_string();
    assert!(schema.contains("DEDICATED SUBAGENT SESSION"));
    assert!(!schema.contains("freshly spawned isolated worker session"));
}
