//! #20: the plan approval gate is never satisfied by the tool auto-approve
//! policy.
//!
//! - With the default `[agent] plan_require_approval = true`, checklist
//!   `init` (create arm AND import arm) waits in Editing even when the tool
//!   context carries auto-approve (yolo / cron / run / a2a).
//! - The explicit escape hatch (`plan_require_approval = false`) restores the
//!   legacy auto-activation and stamps `ApprovalSource::Auto`.
//! - On resume, an Active plan stamped `Auto` demotes back to the approval
//!   queue in `load_plan_from_path` while the gate is on — but unmarked
//!   pre-fix plans (and `User`-stamped ones) are grandfathered and stay
//!   Active.

use crate::brain::tools::plan_tool::PlanTool;
use crate::brain::tools::{Tool, ToolExecutionContext};
use crate::config::profile::{home_for_profile, with_profile_home_async};
use crate::tui::plan::{ApprovalSource, PlanStatus};
use crate::utils::plan_files::{load_plan, load_plan_from_path};
use serde_json::json;
use tempfile::TempDir;
use uuid::Uuid;

/// Run `f` under a throwaway profile home (mirrors the plan_tool_test
/// harness) so plan files and config reads never touch the real
/// `~/.opencrabs/`.
async fn in_temp_home<F, T>(f: F) -> T
where
    F: std::future::Future<Output = T>,
{
    let profile = format!("plan-approval-gate-test-{}", Uuid::new_v4());
    let out = with_profile_home_async(Some(&profile), f).await;
    let home = home_for_profile(Some(&profile));
    let _ = std::fs::remove_dir_all(&home);
    out
}

/// Like `in_temp_home`, but seeds `<home>/config.toml` first so the
/// `Config::load()` inside the future sees the given values.
async fn in_temp_home_with_config<F, T>(config_toml: &str, f: F) -> T
where
    F: std::future::Future<Output = T>,
{
    let profile = format!("plan-approval-gate-test-{}", Uuid::new_v4());
    let home = home_for_profile(Some(&profile));
    std::fs::create_dir_all(&home).expect("create temp profile home");
    std::fs::write(home.join("config.toml"), config_toml).expect("write config");
    std::fs::write(home.join("keys.toml"), "").expect("write keys");
    let out = with_profile_home_async(Some(&profile), f).await;
    let _ = std::fs::remove_dir_all(&home);
    out
}

// ── init arms under auto-approve ─────────────────────────────────

#[tokio::test]
async fn create_arm_auto_approve_waits_in_editing_by_default() {
    in_temp_home(async {
        let ctx = ToolExecutionContext::new(Uuid::new_v4()).with_auto_approve(true);
        let tool = PlanTool;

        let input = json!({
            "operation": "init",
            "title": "Yolo checklist",
            "mode": "checklist",
            "tasks": [{"title": "t1", "description": "d1"}]
        });

        let result = tool.execute(input, &ctx).await.unwrap();
        assert!(result.success, "init failed: {}", result.output);

        let plan = load_plan(ctx.session_id).await.unwrap();
        assert_eq!(
            plan.status,
            PlanStatus::Editing,
            "auto-approve must NOT satisfy plan approval (#20)"
        );
        assert!(
            plan.pending_approval,
            "approval-queue marker must be set so the Approve surface appears"
        );
        assert!(plan.approved_at.is_none());
        assert!(plan.approval_source.is_none());
    })
    .await;
}

#[tokio::test]
async fn escape_hatch_create_arm_auto_activates_and_stamps_auto() {
    in_temp_home_with_config("[agent]\nplan_require_approval = false\n", async {
        let ctx = ToolExecutionContext::new(Uuid::new_v4()).with_auto_approve(true);
        let tool = PlanTool;

        let input = json!({
            "operation": "init",
            "title": "Exempt checklist",
            "mode": "checklist",
            "tasks": [{"title": "t1", "description": "d1"}]
        });

        let result = tool.execute(input, &ctx).await.unwrap();
        assert!(result.success, "init failed: {}", result.output);
        assert!(
            result.output.contains("Active — auto-approve"),
            "escape hatch restores legacy auto-activation: {}",
            result.output
        );

        let plan = load_plan(ctx.session_id).await.unwrap();
        assert_eq!(plan.status, PlanStatus::Active);
        assert!(plan.approved_at.is_some());
        assert_eq!(
            plan.approval_source,
            Some(ApprovalSource::Auto),
            "exempt auto-activation must be attributed so a re-enabled gate \
             can demote it on resume"
        );
    })
    .await;
}

#[tokio::test]
async fn escape_hatch_import_arm_auto_activates_and_stamps_auto() {
    in_temp_home_with_config("[agent]\nplan_require_approval = false\n", async {
        let source = json!({
            "title": "Imported exempt",
            "description": "Escape-hatch import fixture",
            "tasks": [{"title": "t1", "description": "d1", "task_type": "edit"}]
        });
        let dir = TempDir::new().unwrap();
        let import_path = dir.path().join("import.json");
        std::fs::write(&import_path, source.to_string()).unwrap();

        let ctx = ToolExecutionContext::new(Uuid::new_v4()).with_auto_approve(true);
        let tool = PlanTool;
        let input = json!({
            "operation": "init",
            "file_path": import_path.to_string_lossy()
        });

        let result = tool.execute(input, &ctx).await.unwrap();
        assert!(
            result.success,
            "import failed: {}",
            result.error.unwrap_or_default()
        );

        let plan = load_plan(ctx.session_id).await.unwrap();
        assert_eq!(
            plan.status,
            PlanStatus::Active,
            "escape hatch restores legacy import auto-activation"
        );
        assert!(plan.approved_at.is_some());
        assert_eq!(plan.approval_source, Some(ApprovalSource::Auto));
    })
    .await;
}

#[tokio::test]
async fn design_init_still_refused_under_escape_hatch_without_slash() {
    // #20 kept the #581-era rush-refusal ONLY for the escape hatch: with
    // require=false the tool policy would auto-activate a design plan, so an
    // agent-initiated design in yolo (no /plan slash) is still refused toward
    // the checklist track.
    in_temp_home_with_config("[agent]\nplan_require_approval = false\n", async {
        let ctx = ToolExecutionContext::new(Uuid::new_v4()).with_auto_approve(true);
        let tool = PlanTool;
        let result = tool
            .execute(
                json!({ "operation": "init", "title": "Yolo design", "mode": "design" }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(
            !result.success,
            "escape-hatch yolo design must be refused without the slash"
        );
        let err = result.error.unwrap_or_default();
        assert!(
            err.contains("checklist"),
            "refusal names the alternative, got: {err}"
        );
        assert!(
            err.contains("/plan"),
            "refusal names /plan as the review gate, got: {err}"
        );
    })
    .await;
}

// ── resume demotion / grandfathering ─────────────────────────────

/// Write a minimal plan JSON and return its path.
fn write_plan_json(dir: &TempDir, doc: serde_json::Value) -> std::path::PathBuf {
    let path = dir.path().join(".opencrabs_plan_demotion-test.json");
    std::fs::write(&path, doc.to_string()).unwrap();
    path
}

fn plan_doc(title: &str, extra: serde_json::Value) -> serde_json::Value {
    let mut doc = json!({
        "id": Uuid::new_v4().to_string(),
        "session_id": Uuid::new_v4().to_string(),
        "title": title,
        "tasks": [],
        "status": "Active"
    });
    if let (Some(base), Some(patch)) = (doc.as_object_mut(), extra.as_object()) {
        for (k, v) in patch {
            base.insert(k.clone(), v.clone());
        }
    }
    doc
}

#[tokio::test]
async fn resume_demotes_auto_stamped_plan_while_gate_on() {
    in_temp_home(async {
        // No config seeded: plan_require_approval defaults to true.
        let dir = TempDir::new().unwrap();
        let path = write_plan_json(
            &dir,
            plan_doc(
                "escape-hatch activation",
                json!({
                    "approved_at": "2026-08-28T12:00:00Z",
                    "approval_source": "auto"
                }),
            ),
        );

        let plan = load_plan_from_path(&path).expect("plan loads");
        assert_eq!(
            plan.status,
            PlanStatus::Editing,
            "Auto-stamped Active plan must demote while the gate is on (#20)"
        );
        assert!(plan.pending_approval, "demoted plan re-enters the approval queue");
        assert!(plan.approved_at.is_none(), "auto stamp is cleared");
        assert!(plan.approval_source.is_none());

        // The demotion is persisted: a second load sees Editing and does
        // not fire again (a restart cannot resurrect the Active state).
        let again = load_plan_from_path(&path).expect("plan reloads");
        assert_eq!(again.status, PlanStatus::Editing);
        assert!(again.pending_approval);
    })
    .await;
}

#[tokio::test]
async fn resume_keeps_auto_stamped_plan_when_gate_off() {
    in_temp_home_with_config("[agent]\nplan_require_approval = false\n", async {
        let dir = TempDir::new().unwrap();
        let path = write_plan_json(
            &dir,
            plan_doc(
                "exempt activation, gate still off",
                json!({
                    "approved_at": "2026-08-28T12:00:00Z",
                    "approval_source": "auto"
                }),
            ),
        );

        let plan = load_plan_from_path(&path).expect("plan loads");
        assert_eq!(
            plan.status,
            PlanStatus::Active,
            "with the escape hatch on, auto-stamped plans keep executing"
        );
        assert_eq!(plan.approval_source, Some(ApprovalSource::Auto));
    })
    .await;
}

#[tokio::test]
async fn resume_keeps_user_stamped_plan() {
    in_temp_home(async {
        let dir = TempDir::new().unwrap();
        let path = write_plan_json(
            &dir,
            plan_doc(
                "genuinely approved",
                json!({
                    "approved_at": "2026-08-28T12:00:00Z",
                    "approval_source": "user"
                }),
            ),
        );

        let plan = load_plan_from_path(&path).expect("plan loads");
        assert_eq!(plan.status, PlanStatus::Active, "user approval survives resume");
        assert!(plan.approved_at.is_some());
        assert_eq!(plan.approval_source, Some(ApprovalSource::User));
    })
    .await;
}

#[tokio::test]
async fn resume_grandfathers_unmarked_pre_fix_plan() {
    in_temp_home(async {
        // Pre-fix binary shape: approved_at stamped, no approval_source.
        // Indistinguishable from a genuine human approval — grandfathered.
        let dir = TempDir::new().unwrap();
        let path = write_plan_json(
            &dir,
            plan_doc(
                "pre-fix unattributed stamp",
                json!({"approved_at": "2026-08-28T12:00:00Z"}),
            ),
        );

        let plan = load_plan_from_path(&path).expect("plan loads");
        assert_eq!(
            plan.status,
            PlanStatus::Active,
            "unmarked Active plans are grandfathered (#20)"
        );
        assert!(plan.approval_source.is_none());
    })
    .await;
}

#[tokio::test]
async fn resume_keeps_active_plan_without_approved_at() {
    in_temp_home(async {
        let dir = TempDir::new().unwrap();
        let path = write_plan_json(&dir, plan_doc("legacy checklist", json!({})));

        let plan = load_plan_from_path(&path).expect("plan loads");
        assert_eq!(
            plan.status,
            PlanStatus::Active,
            "Active without approved_at predates the stamp and passes"
        );
        assert!(plan.approval_source.is_none());
    })
    .await;
}

// ── serde contract ───────────────────────────────────────────────

#[test]
fn approval_source_serde_roundtrip() {
    assert_eq!(serde_json::to_string(&ApprovalSource::User).unwrap(), "\"user\"");
    assert_eq!(serde_json::to_string(&ApprovalSource::Auto).unwrap(), "\"auto\"");
    let back: ApprovalSource = serde_json::from_str("\"auto\"").unwrap();
    assert_eq!(back, ApprovalSource::Auto);
}
