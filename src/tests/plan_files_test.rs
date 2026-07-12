//! Tests for the shared session plan file store (`utils::plan_files`):
//! lifecycle state derivation, the durable pre-init Editing flag, the
//! legacy seven-status load map (with silent Completed archive and
//! Cancelled delete), canonical status strings on write, the design `.md`
//! scaffold, and the Editing markdown-to-description mirror.

use crate::config::profile::{home_for_profile, with_profile_home_async};
use crate::tui::plan::{PlanDocument, PlanStatus, PlanTask, TaskType};
use crate::utils::plan_files::{
    PlanModeState, archive_dir, create_design_md, discard_plan, is_pre_init_editing, load_plan,
    plan_json_path, plan_md_path, plan_mode_state, save_plan, set_pre_init_editing,
    sync_md_to_json, template_section_warnings,
};
use uuid::Uuid;

/// Run `f` under a throwaway profile home so nothing touches the real
/// `~/.opencrabs/agents/session/`, then clean the profile dir up.
async fn in_temp_home<F, T>(f: F) -> T
where
    F: std::future::Future<Output = T>,
{
    let profile = format!("plan-files-test-{}", Uuid::new_v4());
    let out = with_profile_home_async(Some(&profile), f).await;
    let home = home_for_profile(Some(&profile));
    let _ = std::fs::remove_dir_all(&home);
    out
}

fn task(order: usize, title: &str) -> PlanTask {
    PlanTask::new(order, title.to_string(), "desc".to_string(), TaskType::Edit)
}

fn write_raw_plan(session_id: Uuid, status: &str, task_count: usize) {
    let mut plan = PlanDocument::new(session_id, "Legacy plan".to_string(), String::new());
    for i in 0..task_count {
        plan.add_task(task(i + 1, &format!("t{}", i + 1)));
    }
    save_plan(&plan).unwrap();
    // Rewrite the status string raw, bypassing the canonical serializer.
    let path = plan_json_path(session_id);
    let mut v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    v["status"] = serde_json::Value::String(status.to_string());
    std::fs::write(&path, serde_json::to_string_pretty(&v).unwrap()).unwrap();
}

#[tokio::test]
async fn no_plan_state_when_no_file() {
    in_temp_home(async {
        let sid = Uuid::new_v4();
        assert_eq!(plan_mode_state(sid), PlanModeState::NoPlan);
        assert!(!is_pre_init_editing(sid));
        assert!(load_plan(sid).is_none());
    })
    .await;
}

#[tokio::test]
async fn pre_init_flag_is_durable_and_gates_state() {
    in_temp_home(async {
        let sid = Uuid::new_v4();
        set_pre_init_editing(sid).unwrap();

        // The flag is a durable file, not process state: a fresh read of
        // disk (what a restart does) still sees it.
        assert!(plan_json_path(sid).exists());
        assert!(is_pre_init_editing(sid));
        assert_eq!(plan_mode_state(sid), PlanModeState::PreInitEditing);

        // The sidecar is minimal: no approvable .md, no tasks.
        assert!(!plan_md_path(sid).exists());
        let plan = load_plan(sid).unwrap();
        assert!(plan.pre_init_editing);
        assert!(plan.tasks.is_empty());
        assert_eq!(plan.status, PlanStatus::Editing);

        // Setting it again is idempotent.
        set_pre_init_editing(sid).unwrap();
        assert!(is_pre_init_editing(sid));

        // Discard clears the flag and returns to NoPlan.
        discard_plan(sid);
        assert_eq!(plan_mode_state(sid), PlanModeState::NoPlan);
    })
    .await;
}

#[tokio::test]
async fn pre_init_refused_when_plan_is_live() {
    in_temp_home(async {
        let sid = Uuid::new_v4();
        let mut plan = PlanDocument::new(sid, "Live".to_string(), String::new());
        plan.add_task(task(1, "t1"));
        plan.status = PlanStatus::Active;
        save_plan(&plan).unwrap();

        assert!(set_pre_init_editing(sid).is_err());
        assert_eq!(plan_mode_state(sid), PlanModeState::Active);
    })
    .await;
}

#[tokio::test]
async fn post_init_editing_requires_md() {
    in_temp_home(async {
        let sid = Uuid::new_v4();
        let plan = PlanDocument::new(sid, "Design".to_string(), String::new());
        save_plan(&plan).unwrap();
        create_design_md(sid, "Design").unwrap();

        assert_eq!(plan_mode_state(sid), PlanModeState::PostInitEditing);
        assert!(!is_pre_init_editing(sid));
    })
    .await;
}

#[tokio::test]
async fn save_writes_canonical_status_strings() {
    in_temp_home(async {
        let sid = Uuid::new_v4();
        let mut plan = PlanDocument::new(sid, "Canonical".to_string(), String::new());
        save_plan(&plan).unwrap();
        let raw = std::fs::read_to_string(plan_json_path(sid)).unwrap();
        assert!(raw.contains("\"Editing\""), "got: {raw}");

        plan.status = PlanStatus::Active;
        save_plan(&plan).unwrap();
        let raw = std::fs::read_to_string(plan_json_path(sid)).unwrap();
        assert!(raw.contains("\"Active\""), "got: {raw}");
    })
    .await;
}

#[tokio::test]
async fn legacy_draft_checklist_normalizes_to_active() {
    in_temp_home(async {
        let sid = Uuid::new_v4();
        // Old-world Draft with tasks: executable then, must stay executable
        // now (there is no .md to approve, so Editing would trap it).
        write_raw_plan(sid, "Draft", 2);
        let plan = load_plan(sid).unwrap();
        assert_eq!(plan.status, PlanStatus::Active);
        assert_eq!(plan_mode_state(sid), PlanModeState::Active);
    })
    .await;
}

#[tokio::test]
async fn legacy_approved_and_in_progress_map_to_active() {
    in_temp_home(async {
        for legacy in ["Approved", "InProgress"] {
            let sid = Uuid::new_v4();
            write_raw_plan(sid, legacy, 1);
            let plan = load_plan(sid).unwrap();
            assert_eq!(plan.status, PlanStatus::Active, "legacy {legacy}");
            assert_eq!(plan_mode_state(sid), PlanModeState::Active);
        }
    })
    .await;
}

#[tokio::test]
async fn legacy_completed_archives_silently() {
    in_temp_home(async {
        let sid = Uuid::new_v4();
        write_raw_plan(sid, "Completed", 1);

        assert!(
            load_plan(sid).is_none(),
            "completed plan must resolve to NoPlan"
        );
        assert!(!plan_json_path(sid).exists(), "live JSON must be gone");
        assert_eq!(plan_mode_state(sid), PlanModeState::NoPlan);

        // The plan retired into the archive dir instead of being lost.
        let archived: Vec<_> = std::fs::read_dir(archive_dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains(&sid.to_string()) && n.ends_with(".json"))
            .collect();
        assert_eq!(
            archived.len(),
            1,
            "expected one archived JSON, got {archived:?}"
        );
    })
    .await;
}

#[tokio::test]
async fn legacy_cancelled_deletes() {
    in_temp_home(async {
        let sid = Uuid::new_v4();
        write_raw_plan(sid, "Cancelled", 1);
        assert!(load_plan(sid).is_none());
        assert!(!plan_json_path(sid).exists());
        assert_eq!(plan_mode_state(sid), PlanModeState::NoPlan);
    })
    .await;
}

#[tokio::test]
async fn idle_active_plan_with_empty_tasks_survives_load() {
    in_temp_home(async {
        // A seed-failed Active plan (no tasks yet) must stay intact so the
        // idle retry path can pick it up: loading is never destructive for
        // live statuses.
        let sid = Uuid::new_v4();
        let mut plan = PlanDocument::new(sid, "Seed failed".to_string(), String::new());
        plan.status = PlanStatus::Active;
        save_plan(&plan).unwrap();

        let loaded = load_plan(sid).unwrap();
        assert_eq!(loaded.status, PlanStatus::Active);
        assert!(loaded.tasks.is_empty());
        assert!(plan_json_path(sid).exists());
        assert_eq!(plan_mode_state(sid), PlanModeState::Active);
    })
    .await;
}

#[tokio::test]
async fn design_md_scaffold_and_mirror() {
    in_temp_home(async {
        let sid = Uuid::new_v4();
        let plan = PlanDocument::new(sid, "Design doc".to_string(), String::new());
        save_plan(&plan).unwrap();
        let md_path = create_design_md(sid, "Design doc").unwrap();
        let scaffold = std::fs::read_to_string(&md_path).unwrap();
        assert!(scaffold.starts_with("# Design doc"));
        assert!(scaffold.contains("## Context"));
        assert!(scaffold.contains("## Implementation steps"));

        // Edit the .md, then mirror: the JSON description follows the body
        // and tasks stay empty (Editing cannot persist a checklist).
        let body = "# Design doc\n\n## Context\n- **Problem:** X is broken\n\
                    - **Target state:** X works\n- **Intent:** user asked\n\n\
                    ## Implementation steps\n1. Fix X in module Y\n";
        std::fs::write(&md_path, body).unwrap();
        let warnings = sync_md_to_json(sid);
        assert!(
            warnings.is_empty(),
            "complete template warned: {warnings:?}"
        );

        let mirrored = load_plan(sid).unwrap();
        assert_eq!(mirrored.description, body);
        assert!(mirrored.tasks.is_empty());
        assert_eq!(mirrored.status, PlanStatus::Editing);
    })
    .await;
}

#[test]
fn template_warnings_flag_missing_sections() {
    let empty = template_section_warnings("just prose, no structure");
    assert!(empty.iter().any(|w| w.contains("## Context")));
    assert!(empty.iter().any(|w| w.contains("## Implementation steps")));

    // Labels present but unfilled still warn.
    let unfilled = "## Context\n- **Problem:** \n- **Target state:** \n- **Intent:** \n\
                    \n## Implementation steps\n1. \n";
    let w = template_section_warnings(unfilled);
    assert!(w.iter().any(|x| x.contains("**Problem:**")));
    assert!(w.iter().any(|x| x.contains("numbered step")));

    // A filled template is quiet.
    let filled = "## Context\n- **Problem:** broken\n- **Target state:** fixed\n\
                  - **Intent:** asked\n\n## Implementation steps\n1. do the thing\n";
    assert!(template_section_warnings(filled).is_empty());
}
