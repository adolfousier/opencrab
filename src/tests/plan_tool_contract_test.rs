//! Contract tests for the plan tool's lifecycle-engine behavior:
//! mode disambiguation (design vs checklist), pre-init upgrade/replace
//! rules, re-init refusal while a plan is live, Active-only checklist
//! operations, `add_tasks` plus the `add_task` alias, import rules,
//! archive on last complete, and the removal of auto-approve on first
//! `start`.

use crate::brain::tools::plan_tool::PlanTool;
use crate::brain::tools::{Tool, ToolExecutionContext};
use crate::config::profile::{home_for_profile, with_profile_home_async};
use crate::tui::plan::PlanStatus;
use crate::utils::plan_files::{
    PlanModeState, load_plan, plan_json_path, plan_md_path, plan_mode_state, set_pre_init_editing,
};
use serde_json::json;

async fn in_temp_home<F, T>(f: F) -> T
where
    F: std::future::Future<Output = T>,
{
    let profile = format!("plan-contract-test-{}", uuid::Uuid::new_v4());
    let out = with_profile_home_async(Some(&profile), f).await;
    let home = home_for_profile(Some(&profile));
    let _ = std::fs::remove_dir_all(&home);
    out
}

async fn run(
    tool: &PlanTool,
    ctx: &ToolExecutionContext,
    input: serde_json::Value,
) -> (bool, String) {
    let r = tool.execute(input, ctx).await.unwrap();
    let text = if r.success {
        r.output
    } else {
        r.error.unwrap_or_default()
    };
    (r.success, text)
}

#[tokio::test]
async fn design_init_creates_md_and_enters_editing() {
    in_temp_home(async {
        let tool = PlanTool;
        let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
        let (ok, out) = run(
            &tool,
            &ctx,
            json!({ "operation": "init", "title": "Refactor auth", "mode": "design" }),
        )
        .await;
        assert!(ok, "design init must succeed, got: {out}");
        let md = plan_md_path(ctx.session_id).await;
        assert!(md.exists(), "design init must create the session .md");
        assert!(
            out.contains(&md.display().to_string()),
            "result must return the absolute .md path, got: {out}"
        );
        assert!(
            out.contains("Do NOT call 'start'"),
            "design init must steer the model away from start, got: {out}"
        );
        assert_eq!(
            plan_mode_state(ctx.session_id).await,
            PlanModeState::PostInitEditing
        );
        let plan = load_plan(ctx.session_id).await.unwrap();
        assert_eq!(plan.status, PlanStatus::Editing);
        assert!(plan.tasks.is_empty());
        assert!(plan.approved_at.is_none());
    })
    .await;
}

#[tokio::test]
async fn omitted_mode_disambiguates_by_tasks() {
    in_temp_home(async {
        let tool = PlanTool;
        // No tasks → design.
        let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
        let (ok, _) = run(&tool, &ctx, json!({ "operation": "init", "title": "D" })).await;
        assert!(ok);
        assert_eq!(
            plan_mode_state(ctx.session_id).await,
            PlanModeState::PostInitEditing
        );

        // Tasks → checklist, Active immediately.
        let ctx2 = ToolExecutionContext::new(uuid::Uuid::new_v4());
        let (ok, out) = run(
            &tool,
            &ctx2,
            json!({ "operation": "init", "title": "C", "tasks": [{ "title": "t1" }] }),
        )
        .await;
        assert!(ok, "checklist init must succeed, got: {out}");
        assert!(
            out.contains("Active"),
            "checklist init reports Active, got: {out}"
        );
        assert_eq!(
            plan_mode_state(ctx2.session_id).await,
            PlanModeState::Active
        );
    })
    .await;
}

#[tokio::test]
async fn conflicting_mode_and_tasks_are_refused() {
    in_temp_home(async {
        let tool = PlanTool;
        let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
        let (ok, msg) = run(
            &tool,
            &ctx,
            json!({ "operation": "init", "title": "X", "mode": "design", "tasks": [{ "title": "t" }] }),
        )
        .await;
        assert!(!ok, "design with tasks must be refused");
        assert!(msg.contains("design"), "got: {msg}");

        let (ok, msg) = run(
            &tool,
            &ctx,
            json!({ "operation": "init", "title": "X", "mode": "checklist" }),
        )
        .await;
        assert!(!ok, "checklist without tasks must be refused");
        assert!(msg.contains("checklist"), "got: {msg}");

        // Neither refusal left plan artifacts behind.
        assert_eq!(plan_mode_state(ctx.session_id).await, PlanModeState::NoPlan);
    })
    .await;
}

#[tokio::test]
async fn design_init_refused_under_auto_approve() {
    in_temp_home(async {
        let tool = PlanTool;
        let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4()).with_auto_approve(true);
        let (ok, msg) = run(
            &tool,
            &ctx,
            json!({ "operation": "init", "title": "Yolo design", "mode": "design" }),
        )
        .await;
        assert!(!ok, "yolo plus design must be refused");
        assert!(
            msg.contains("checklist"),
            "refusal names the alternative, got: {msg}"
        );

        // Checklist stays allowed under auto-approve.
        let (ok, _) = run(
            &tool,
            &ctx,
            json!({ "operation": "init", "title": "Yolo checklist", "tasks": [{ "title": "t" }] }),
        )
        .await;
        assert!(ok, "yolo checklist init must succeed");
    })
    .await;
}

#[tokio::test]
async fn pre_init_upgrades_to_design_and_replaces_for_checklist() {
    in_temp_home(async {
        let tool = PlanTool;

        // Upgrade: pre-init → design init → post-init Editing.
        let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
        set_pre_init_editing(ctx.session_id).await.unwrap();
        let (ok, _) = run(
            &tool,
            &ctx,
            json!({ "operation": "init", "title": "Upgraded", "mode": "design" }),
        )
        .await;
        assert!(ok, "design init from pre-init must upgrade the sidecar");
        assert_eq!(
            plan_mode_state(ctx.session_id).await,
            PlanModeState::PostInitEditing
        );
        let plan = load_plan(ctx.session_id).await.unwrap();
        assert!(!plan.pre_init_editing, "the flag is consumed by init");

        // Replace: pre-init → checklist init → Active, no /discard needed.
        let ctx2 = ToolExecutionContext::new(uuid::Uuid::new_v4());
        set_pre_init_editing(ctx2.session_id).await.unwrap();
        let (ok, _) = run(
            &tool,
            &ctx2,
            json!({ "operation": "init", "title": "Changed my mind", "tasks": [{ "title": "t" }] }),
        )
        .await;
        assert!(ok, "checklist init from pre-init must replace the flag");
        assert_eq!(
            plan_mode_state(ctx2.session_id).await,
            PlanModeState::Active
        );
    })
    .await;
}

#[tokio::test]
async fn reinit_refused_while_plan_is_live() {
    in_temp_home(async {
        let tool = PlanTool;

        // Post-init Editing blocks init.
        let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
        run(
            &tool,
            &ctx,
            json!({ "operation": "init", "title": "First" }),
        )
        .await;
        let (ok, msg) = run(
            &tool,
            &ctx,
            json!({ "operation": "init", "title": "Second" }),
        )
        .await;
        assert!(!ok, "re-init over post-init Editing must be refused");
        assert!(msg.to_lowercase().contains("discard"), "got: {msg}");

        // Active blocks init too.
        let ctx2 = ToolExecutionContext::new(uuid::Uuid::new_v4());
        run(
            &tool,
            &ctx2,
            json!({ "operation": "init", "title": "Live", "tasks": [{ "title": "t" }] }),
        )
        .await;
        let (ok, msg) = run(
            &tool,
            &ctx2,
            json!({ "operation": "init", "title": "Second" }),
        )
        .await;
        assert!(!ok, "re-init over Active must be refused");
        assert!(msg.to_lowercase().contains("discard"), "got: {msg}");
    })
    .await;
}

#[tokio::test]
async fn checklist_ops_blocked_while_editing() {
    in_temp_home(async {
        let tool = PlanTool;
        let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
        run(
            &tool,
            &ctx,
            json!({ "operation": "init", "title": "Design" }),
        )
        .await;

        for op in [
            json!({ "operation": "start" }),
            json!({ "operation": "complete", "task_order": 1 }),
            json!({ "operation": "add_tasks", "tasks": [{ "title": "t" }] }),
            json!({ "operation": "add_task", "title": "t" }),
        ] {
            let (ok, msg) = run(&tool, &ctx, op.clone()).await;
            assert!(!ok, "{op} must be refused while Editing");
            assert!(
                msg.contains("Editing") || msg.contains("approve"),
                "refusal must explain the Editing block, got: {msg}"
            );
        }
    })
    .await;
}

#[tokio::test]
async fn add_tasks_appends_multiple_and_alias_still_works() {
    in_temp_home(async {
        let tool = PlanTool;
        let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
        run(
            &tool,
            &ctx,
            json!({ "operation": "init", "title": "List", "tasks": [{ "title": "one" }] }),
        )
        .await;

        let (ok, out) = run(
            &tool,
            &ctx,
            json!({ "operation": "add_tasks", "tasks": [{ "title": "two" }, { "title": "three" }] }),
        )
        .await;
        assert!(ok, "add_tasks must succeed, got: {out}");
        assert!(out.contains("2 task(s)") && out.contains("3 tasks"), "got: {out}");

        let (ok, out) = run(&tool, &ctx, json!({ "operation": "add_task", "title": "four" })).await;
        assert!(ok, "add_task alias must keep working, got: {out}");
        assert_eq!(load_plan(ctx.session_id).await.unwrap().tasks.len(), 4);

        let (ok, msg) = run(&tool, &ctx, json!({ "operation": "add_tasks", "tasks": [] })).await;
        assert!(!ok, "empty add_tasks must be refused, got: {msg}");
    })
    .await;
}

#[tokio::test]
async fn first_start_does_not_auto_approve() {
    in_temp_home(async {
        let tool = PlanTool;
        let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
        run(
            &tool,
            &ctx,
            json!({ "operation": "init", "title": "NoAutoApprove", "tasks": [{ "title": "t" }] }),
        )
        .await;
        let (ok, _) = run(&tool, &ctx, json!({ "operation": "start" })).await;
        assert!(ok);
        let plan = load_plan(ctx.session_id).await.unwrap();
        assert!(
            plan.approved_at.is_none(),
            "start must not stamp approved_at: that belongs to user Approve"
        );
    })
    .await;
}

#[tokio::test]
async fn completing_last_task_archives_to_no_plan() {
    in_temp_home(async {
        let tool = PlanTool;
        let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
        run(
            &tool,
            &ctx,
            json!({ "operation": "init", "title": "Short", "tasks": [{ "title": "only" }] }),
        )
        .await;
        run(&tool, &ctx, json!({ "operation": "start" })).await;
        let (ok, out) = run(
            &tool,
            &ctx,
            json!({ "operation": "complete", "task_order": 1, "action": "success" }),
        )
        .await;
        assert!(ok);
        assert!(
            out.contains("archived"),
            "completion reports the archive, got: {out}"
        );
        assert!(
            !plan_json_path(ctx.session_id).await.exists(),
            "live JSON must be archived away"
        );
        assert_eq!(plan_mode_state(ctx.session_id).await, PlanModeState::NoPlan);
    })
    .await;
}

#[tokio::test]
async fn empty_import_is_refused() {
    in_temp_home(async {
        let tool = PlanTool;
        let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("empty-plan.json");
        std::fs::write(
            &file,
            serde_json::to_string(&json!({
                "title": "Empty",
                "description": "no tasks",
                "tasks": []
            }))
            .unwrap(),
        )
        .unwrap();

        let (ok, msg) = run(
            &tool,
            &ctx,
            json!({ "operation": "init", "file_path": file.to_str().unwrap() }),
        )
        .await;
        assert!(!ok, "empty import must be refused");
        assert!(msg.contains("no tasks"), "got: {msg}");
        assert_eq!(plan_mode_state(ctx.session_id).await, PlanModeState::NoPlan);
    })
    .await;
}

#[tokio::test]
async fn import_refused_while_live_but_replaces_pre_init() {
    in_temp_home(async {
        let tool = PlanTool;
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("import-plan.json");
        std::fs::write(
            &file,
            serde_json::to_string(&json!({
                "title": "Imported",
                "description": "structured",
                "tasks": [{ "title": "t1", "description": "d", "task_type": "edit" }]
            }))
            .unwrap(),
        )
        .unwrap();

        // From pre-init: import replaces the flag and goes Active.
        let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
        set_pre_init_editing(ctx.session_id).await.unwrap();
        let (ok, _) = run(
            &tool,
            &ctx,
            json!({ "operation": "init", "file_path": file.to_str().unwrap() }),
        )
        .await;
        assert!(ok, "import from pre-init must replace the flag");
        assert_eq!(plan_mode_state(ctx.session_id).await, PlanModeState::Active);

        // From post-init Editing: refused.
        let ctx2 = ToolExecutionContext::new(uuid::Uuid::new_v4());
        run(
            &tool,
            &ctx2,
            json!({ "operation": "init", "title": "Design" }),
        )
        .await;
        let (ok, msg) = run(
            &tool,
            &ctx2,
            json!({ "operation": "init", "file_path": file.to_str().unwrap() }),
        )
        .await;
        assert!(
            !ok,
            "import over post-init Editing must be refused, got: {msg}"
        );
    })
    .await;
}
