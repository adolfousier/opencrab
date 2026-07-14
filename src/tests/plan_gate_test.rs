//! Dedicated coverage for the plan-mode write/bash gate
//! (`brain::tools::plan_gate`), the highest-risk surface of the plan
//! lifecycle engine. Verifies, per state:
//!
//! - pre-init Editing denies project writes but allows exploratory bash
//!   and the plan tool
//! - post-init Editing hard-denies all bash, allows writes ONLY to the
//!   session `.md`, and denies other `~/.opencrabs` writes
//! - Active freezes the `.md` against generic write tools
//! - NoPlan gates nothing

use crate::brain::tools::plan_gate::check_plan_gate;
use crate::brain::tools::r#trait::ToolCapability;
use crate::config::profile::{home_for_profile, with_profile_home_async};
use crate::tui::plan::{PlanDocument, PlanStatus, PlanTask, TaskType};
use crate::utils::plan_files::{create_design_md, plan_md_path, save_plan, set_pre_init_editing};
use serde_json::json;
use uuid::Uuid;

const WRITE: &[ToolCapability] = &[
    ToolCapability::ReadFiles,
    ToolCapability::WriteFiles,
    ToolCapability::SystemModification,
];
const BASH: &[ToolCapability] = &[
    ToolCapability::ExecuteShell,
    ToolCapability::SystemModification,
    ToolCapability::Network,
];
const CODE_EXEC: &[ToolCapability] = &[
    ToolCapability::ExecuteShell,
    ToolCapability::SystemModification,
    ToolCapability::WriteFiles,
];
const READ: &[ToolCapability] = &[ToolCapability::ReadFiles];
const NETWORK: &[ToolCapability] = &[ToolCapability::Network];
const SYSTEM: &[ToolCapability] = &[ToolCapability::SystemModification];
const PLAN: &[ToolCapability] = &[ToolCapability::PlanManagement];

async fn in_temp_home<F, T>(f: F) -> T
where
    F: std::future::Future<Output = T>,
{
    let profile = format!("plan-gate-test-{}", Uuid::new_v4());
    let out = with_profile_home_async(Some(&profile), f).await;
    let home = home_for_profile(Some(&profile));
    let _ = std::fs::remove_dir_all(&home);
    out
}

async fn make_post_init_editing(sid: Uuid) {
    let plan = PlanDocument::new(sid, "Design".to_string());
    save_plan(&plan).await.unwrap();
    create_design_md(sid, "Design").await.unwrap();
}

async fn make_active(sid: Uuid, with_md: bool) {
    let mut plan = PlanDocument::new(sid, "Exec".to_string());
    let mut task = PlanTask::new(1, "t1".to_string(), "d".to_string(), TaskType::Edit);
    // A started checklist: the seed window (all tasks Pending) has its own
    // stricter policy, covered separately below.
    task.start();
    plan.add_task(task);
    plan.status = PlanStatus::Active;
    save_plan(&plan).await.unwrap();
    if with_md {
        create_design_md(sid, "Exec").await.unwrap();
    }
}

#[tokio::test]
async fn no_plan_gates_nothing() {
    in_temp_home(async {
        let sid = Uuid::new_v4();
        for (name, caps) in [
            ("edit_file", WRITE),
            ("bash", BASH),
            ("telegram_send", NETWORK),
            ("spawn_agent", SYSTEM),
        ] {
            assert!(
                check_plan_gate(sid, name, caps, &json!({})).await.is_none(),
                "{name} must pass with no plan"
            );
        }
    })
    .await;
}

#[tokio::test]
async fn pre_init_denies_writes_allows_bash_and_plan() {
    in_temp_home(async {
        let sid = Uuid::new_v4();
        set_pre_init_editing(sid).await.unwrap();

        // Project writes are denied: there is nothing approvable to write.
        let deny = check_plan_gate(sid, "edit_file", WRITE, &json!({"path": "/tmp/x.rs"})).await;
        assert!(deny.is_some(), "project write must be denied pre-init");

        // Brain-file writes are writes too.
        let deny = check_plan_gate(
            sid,
            "write_opencrabs_file",
            &[ToolCapability::WriteFiles],
            &json!({"path": "MEMORY.md"}),
        )
        .await;
        assert!(deny.is_some(), "opencrabs write must be denied pre-init");

        // Exploratory bash and code execution stay available.
        assert!(
            check_plan_gate(sid, "bash", BASH, &json!({"command": "ls"}))
                .await
                .is_none()
        );
        assert!(
            check_plan_gate(sid, "execute_code", CODE_EXEC, &json!({}))
                .await
                .is_none()
        );

        // Reads, search, and the plan tool flow through.
        assert!(
            check_plan_gate(sid, "read_file", READ, &json!({}))
                .await
                .is_none()
        );
        assert!(
            check_plan_gate(sid, "web_search", NETWORK, &json!({}))
                .await
                .is_none()
        );
        assert!(
            check_plan_gate(sid, "plan", PLAN, &json!({"operation": "init"}))
                .await
                .is_none()
        );

        // Sends, browser mutators, spawn, and system tools are blocked.
        for name in ["telegram_send", "browser_click", "spawn_agent"] {
            assert!(
                check_plan_gate(sid, name, NETWORK, &json!({}))
                    .await
                    .is_some(),
                "{name} must be denied pre-init"
            );
        }
        assert!(
            check_plan_gate(sid, "evolve", SYSTEM, &json!({}))
                .await
                .is_some()
        );
    })
    .await;
}

#[tokio::test]
async fn post_init_denies_bash_and_gates_writes_to_md() {
    in_temp_home(async {
        let sid = Uuid::new_v4();
        make_post_init_editing(sid).await;
        let md = plan_md_path(sid).await;

        // All bash is hard-denied, including write-capable code execution.
        assert!(
            check_plan_gate(sid, "bash", BASH, &json!({"command": "ls"}))
                .await
                .is_some()
        );
        assert!(
            check_plan_gate(sid, "execute_code", CODE_EXEC, &json!({}))
                .await
                .is_some()
        );

        // The session .md is the ONLY writable file.
        let ok = check_plan_gate(
            sid,
            "write_file",
            WRITE,
            &json!({"path": md.to_string_lossy()}),
        )
        .await;
        assert!(ok.is_none(), "session .md write must pass, got: {ok:?}");

        // edit_file on the .md passes too (path key is shared).
        assert!(
            check_plan_gate(
                sid,
                "edit_file",
                WRITE,
                &json!({"path": md.to_string_lossy()})
            )
            .await
            .is_none()
        );

        // Project writes fail.
        assert!(
            check_plan_gate(
                sid,
                "edit_file",
                WRITE,
                &json!({"path": "/tmp/project/main.rs"})
            )
            .await
            .is_some()
        );

        // Other ~/.opencrabs writes fail (relative write_opencrabs_file
        // path resolves under home and is not the .md).
        assert!(
            check_plan_gate(
                sid,
                "write_opencrabs_file",
                &[ToolCapability::WriteFiles],
                &json!({"path": "MEMORY.md"})
            )
            .await
            .is_some()
        );

        // A write tool with no recognizable target is denied (safe default).
        assert!(
            check_plan_gate(sid, "generate_document", WRITE, &json!({}))
                .await
                .is_some()
        );

        // Reads, search, plan, and follow_up_question flow through.
        assert!(
            check_plan_gate(sid, "read_file", READ, &json!({}))
                .await
                .is_none()
        );
        assert!(
            check_plan_gate(sid, "grep", READ, &json!({}))
                .await
                .is_none()
        );
        assert!(
            check_plan_gate(sid, "plan", PLAN, &json!({"operation": "init"}))
                .await
                .is_none()
        );
        assert!(
            check_plan_gate(sid, "follow_up_question", &[], &json!({}))
                .await
                .is_none()
        );

        // Sends, browser mutators, spawn, and system tools are blocked.
        assert!(
            check_plan_gate(sid, "slack_send", NETWORK, &json!({}))
                .await
                .is_some()
        );
        assert!(
            check_plan_gate(sid, "browser_eval", NETWORK, &json!({}))
                .await
                .is_some()
        );
        assert!(
            check_plan_gate(sid, "resume_agent", SYSTEM, &json!({}))
                .await
                .is_some()
        );
        assert!(
            check_plan_gate(sid, "cron_manage", SYSTEM, &json!({}))
                .await
                .is_some()
        );
    })
    .await;
}

#[tokio::test]
async fn active_freezes_md_only() {
    in_temp_home(async {
        let sid = Uuid::new_v4();
        make_active(sid, true).await;
        let md = plan_md_path(sid).await;

        // The design .md is frozen against generic write tools.
        assert!(
            check_plan_gate(
                sid,
                "edit_file",
                WRITE,
                &json!({"path": md.to_string_lossy()})
            )
            .await
            .is_some(),
            "Active .md write must be frozen"
        );

        // Everything else follows the normal approval policy.
        assert!(
            check_plan_gate(
                sid,
                "edit_file",
                WRITE,
                &json!({"path": "/tmp/project/main.rs"})
            )
            .await
            .is_none()
        );
        assert!(
            check_plan_gate(sid, "bash", BASH, &json!({"command": "ls"}))
                .await
                .is_none()
        );
        assert!(
            check_plan_gate(sid, "telegram_send", NETWORK, &json!({}))
                .await
                .is_none()
        );
        assert!(
            check_plan_gate(sid, "spawn_agent", SYSTEM, &json!({}))
                .await
                .is_none()
        );
    })
    .await;
}

#[tokio::test]
async fn active_checklist_without_md_gates_nothing_on_writes() {
    in_temp_home(async {
        // Checklist-track plans have no design .md; nothing to freeze.
        let sid = Uuid::new_v4();
        make_active(sid, false).await;
        assert!(
            check_plan_gate(
                sid,
                "edit_file",
                WRITE,
                &json!({"path": "/tmp/project/main.rs"})
            )
            .await
            .is_none()
        );
        assert!(
            check_plan_gate(sid, "bash", BASH, &json!({"command": "ls"}))
                .await
                .is_none()
        );
    })
    .await;
}

#[tokio::test]
async fn seed_window_blocks_mutators_allows_plan_and_reads() {
    in_temp_home(async {
        // Approved design plan whose checklist has not started yet (empty
        // tasks): only reads and the plan tool flow through.
        let sid = Uuid::new_v4();
        let mut plan = PlanDocument::new(sid, "Seeding".to_string());
        plan.status = PlanStatus::Active;
        save_plan(&plan).await.unwrap();
        create_design_md(sid, "Seeding").await.unwrap();

        assert!(
            check_plan_gate(sid, "plan", PLAN, &json!({"operation": "add_tasks"}))
                .await
                .is_none()
        );
        assert!(
            check_plan_gate(sid, "read_file", READ, &json!({}))
                .await
                .is_none()
        );
        assert!(
            check_plan_gate(sid, "bash", BASH, &json!({"command": "ls"}))
                .await
                .is_some()
        );
        assert!(
            check_plan_gate(sid, "edit_file", WRITE, &json!({"path": "/tmp/p/main.rs"}))
                .await
                .is_some()
        );
        assert!(
            check_plan_gate(sid, "spawn_agent", SYSTEM, &json!({}))
                .await
                .is_some()
        );

        // Partial seed (tasks added, none started) stays blocked too.
        let mut plan = crate::utils::plan_files::load_plan(sid).await.unwrap();
        plan.add_task(PlanTask::new(
            1,
            "t1".to_string(),
            "d".to_string(),
            TaskType::Edit,
        ));
        save_plan(&plan).await.unwrap();
        assert!(
            check_plan_gate(sid, "bash", BASH, &json!({"command": "ls"}))
                .await
                .is_some()
        );

        // Once a task starts, the seed window closes and normal Active
        // policy applies (only the .md stays frozen).
        let mut plan = crate::utils::plan_files::load_plan(sid).await.unwrap();
        plan.tasks[0].start();
        save_plan(&plan).await.unwrap();
        assert!(
            check_plan_gate(sid, "bash", BASH, &json!({"command": "ls"}))
                .await
                .is_none()
        );
        assert!(
            check_plan_gate(sid, "edit_file", WRITE, &json!({"path": "/tmp/p/main.rs"}))
                .await
                .is_none()
        );
    })
    .await;
}
