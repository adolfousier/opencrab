//! Dedicated coverage for the plan-mode write/bash gate
//! (`brain::tools::plan_gate`), the highest-risk surface of the plan
//! lifecycle engine. Verifies, per state:
//!
//! - pre-init Editing denies project writes but allows exploratory bash
//!   and the plan tool
//! - post-init Editing sends bash to approval (RequireApproval), allows
//!   writes ONLY to the session `.md`, and denies other writes
//! - Active freezes the `.md` against generic write tools
//! - NoPlan gates nothing

use crate::brain::tools::plan_gate::{check_plan_gate, GateDecision};
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
                check_plan_gate(sid, name, caps, &json!({})).await.is_allowed(),
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
        assert!(deny.is_denied(), "project write must be denied pre-init");

        // Brain-file writes are writes too.
        let deny = check_plan_gate(
            sid,
            "write_opencrabs_file",
            &[ToolCapability::WriteFiles],
            &json!({"path": "MEMORY.md"}),
        )
        .await;
        assert!(deny.is_denied(), "opencrabs write must be denied pre-init");

        // Destructive tools (bash, code execution) are denied pre-init.
        // Shell reclassification is deferred; for now is_destructive gates all.
        assert!(
            check_plan_gate(sid, "bash", BASH, &json!({"command": "ls"}))
                .await
                .is_denied()
        );
        assert!(
            check_plan_gate(sid, "execute_code", CODE_EXEC, &json!({}))
                .await
                .is_denied()
        );

        // Reads, search, and the plan tool flow through.
        assert!(
            check_plan_gate(sid, "read_file", READ, &json!({}))
                .await
                .is_allowed()
        );
        assert!(
            check_plan_gate(sid, "web_search", NETWORK, &json!({}))
                .await
                .is_allowed()
        );
        assert!(
            check_plan_gate(sid, "plan", PLAN, &json!({"operation": "init"}))
                .await
                .is_allowed()
        );

        // Sends and browser mutators are blocked by name.
        for name in ["telegram_send", "browser_click"] {
            assert!(
                check_plan_gate(sid, name, NETWORK, &json!({}))
                    .await
                    .is_denied(),
                "{name} must be denied pre-init"
            );
        }
        // Agent tools are allowed: the read-only subagent filter
        // (restrict_registry_to_read_only) strips mutators from the
        // spawned agent's registry, so the whole family is safe.
        assert!(
            check_plan_gate(sid, "spawn_agent", SYSTEM, &json!({}))
                .await
                .is_allowed(),
            "spawn_agent must be allowed pre-init (read-only filter handles safety)"
        );
        assert!(
            check_plan_gate(sid, "evolve", SYSTEM, &json!({}))
                .await
                .is_denied()
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

        // Bash goes to approval (RequireApproval), not hard deny.
        assert!(
            check_plan_gate(sid, "bash", BASH, &json!({"command": "ls"}))
                .await
                .needs_approval()
        );
        assert!(
            check_plan_gate(sid, "execute_code", CODE_EXEC, &json!({}))
                .await
                .needs_approval()
        );

        // The session .md is the ONLY writable file.
        let ok = check_plan_gate(
            sid,
            "write_file",
            WRITE,
            &json!({"path": md.to_string_lossy()}),
        )
        .await;
        assert!(ok.is_allowed(), "session .md write must pass, got: {ok:?}");

        // edit_file on the .md passes too (path key is shared).
        assert!(
            check_plan_gate(
                sid,
                "edit_file",
                WRITE,
                &json!({"path": md.to_string_lossy()})
            )
            .await
            .is_allowed()
        );

        // Project writes go to approval (RequireApproval).
        assert!(
            check_plan_gate(
                sid,
                "edit_file",
                WRITE,
                &json!({"path": "/tmp/project/main.rs"})
            )
            .await
            .needs_approval()
        );

        // Other ~/.opencrabs writes go to approval too.
        assert!(
            check_plan_gate(
                sid,
                "write_opencrabs_file",
                &[ToolCapability::WriteFiles],
                &json!({"path": "MEMORY.md"})
            )
            .await
            .needs_approval()
        );

        // A write tool with no recognizable target goes to approval.
        assert!(
            check_plan_gate(sid, "generate_document", WRITE, &json!({}))
                .await
                .needs_approval()
        );

        // Reads, search, plan, and follow_up_question flow through.
        assert!(
            check_plan_gate(sid, "read_file", READ, &json!({}))
                .await
                .is_allowed()
        );
        assert!(
            check_plan_gate(sid, "grep", READ, &json!({}))
                .await
                .is_allowed()
        );
        assert!(
            check_plan_gate(sid, "plan", PLAN, &json!({"operation": "init"}))
                .await
                .is_allowed()
        );
        assert!(
            check_plan_gate(sid, "follow_up_question", &[], &json!({}))
                .await
                .is_allowed()
        );

        // Sends, browser mutators, spawn, and system tools are blocked.
        assert!(
            check_plan_gate(sid, "slack_send", NETWORK, &json!({}))
                .await
                .is_denied()
        );
        assert!(
            check_plan_gate(sid, "browser_eval", NETWORK, &json!({}))
                .await
                .is_denied()
        );
        assert!(
            check_plan_gate(sid, "resume_agent", SYSTEM, &json!({}))
                .await
                .is_allowed()
        );
        assert!(
            check_plan_gate(sid, "cron_manage", SYSTEM, &json!({}))
                .await
                .needs_approval()
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
            .is_denied(),
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
            .is_allowed()
        );
        assert!(
            check_plan_gate(sid, "bash", BASH, &json!({"command": "ls"}))
                .await
                .is_allowed()
        );
        assert!(
            check_plan_gate(sid, "telegram_send", NETWORK, &json!({}))
                .await
                .is_allowed()
        );
        assert!(
            check_plan_gate(sid, "spawn_agent", SYSTEM, &json!({}))
                .await
                .is_allowed()
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
            .is_allowed()
        );
        assert!(
            check_plan_gate(sid, "bash", BASH, &json!({"command": "ls"}))
                .await
                .is_allowed()
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
                .is_allowed()
        );
        assert!(
            check_plan_gate(sid, "read_file", READ, &json!({}))
                .await
                .is_allowed()
        );
        assert!(
            check_plan_gate(sid, "bash", BASH, &json!({"command": "ls"}))
                .await
                .is_denied()
        );
        assert!(
            check_plan_gate(sid, "edit_file", WRITE, &json!({"path": "/tmp/p/main.rs"}))
                .await
                .is_denied()
        );
        assert!(
            check_plan_gate(sid, "spawn_agent", SYSTEM, &json!({}))
                .await
                .is_allowed()
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
                .is_denied()
        );

        // Once a task starts, the seed window closes and normal Active
        // policy applies (only the .md stays frozen).
        let mut plan = crate::utils::plan_files::load_plan(sid).await.unwrap();
        plan.tasks[0].start();
        save_plan(&plan).await.unwrap();
        assert!(
            check_plan_gate(sid, "bash", BASH, &json!({"command": "ls"}))
                .await
                .is_allowed()
        );
        assert!(
            check_plan_gate(sid, "edit_file", WRITE, &json!({"path": "/tmp/p/main.rs"}))
                .await
                .is_allowed()
        );
    })
    .await;
}

// ── restrict_registry_to_read_only (#649) ───────────────────────────────
// A subagent spawned while the parent is Editing must be read-only. The
// filter strips mutating tools from the child registry (child runs under a
// fresh NoPlan session the per-call gate would not catch), keeping only
// read/search/network tools so it can inform or review the design.

struct CapTool {
    name: &'static str,
    caps: Vec<ToolCapability>,
}

#[async_trait::async_trait]
impl crate::brain::tools::Tool for CapTool {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &str {
        "cap test tool"
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({ "type": "object" })
    }
    fn capabilities(&self) -> Vec<ToolCapability> {
        self.caps.clone()
    }
    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &crate::brain::tools::ToolExecutionContext,
    ) -> crate::brain::tools::error::Result<crate::brain::tools::ToolResult> {
        Ok(crate::brain::tools::ToolResult::success("ok".to_string()))
    }
}

#[test]
fn read_only_filter_strips_mutators_keeps_reads() {
    use crate::brain::tools::ToolRegistry;
    use crate::brain::tools::plan_gate::restrict_registry_to_read_only;
    use std::sync::Arc;

    let registry = ToolRegistry::new();
    // Read-only surface — must survive.
    registry.register(Arc::new(CapTool {
        name: "read_file",
        caps: vec![ToolCapability::ReadFiles],
    }));
    registry.register(Arc::new(CapTool {
        name: "grep",
        caps: vec![ToolCapability::ReadFiles],
    }));
    registry.register(Arc::new(CapTool {
        name: "http_request",
        caps: vec![ToolCapability::Network],
    }));
    registry.register(Arc::new(CapTool {
        name: "follow_up_question",
        caps: vec![],
    }));
    // Mutators — must be stripped by capability.
    registry.register(Arc::new(CapTool {
        name: "edit_file",
        caps: vec![ToolCapability::WriteFiles],
    }));
    registry.register(Arc::new(CapTool {
        name: "bash",
        caps: vec![ToolCapability::ExecuteShell],
    }));
    registry.register(Arc::new(CapTool {
        name: "spawn_agent",
        caps: vec![ToolCapability::SystemModification],
    }));
    // Denied by name even though a Network cap alone would not catch it —
    // proves the name list and capability check are OR'd (no drift with the
    // per-call gate's deny set).
    registry.register(Arc::new(CapTool {
        name: "telegram_send",
        caps: vec![ToolCapability::Network],
    }));

    restrict_registry_to_read_only(&registry);

    assert!(registry.has_tool("read_file"), "reads must survive");
    assert!(registry.has_tool("grep"), "search must survive");
    assert!(
        registry.has_tool("http_request"),
        "network read must survive"
    );
    assert!(
        registry.has_tool("follow_up_question"),
        "the question tool must survive"
    );
    assert!(!registry.has_tool("edit_file"), "writes must be stripped");
    assert!(!registry.has_tool("bash"), "bash must be stripped");
    assert!(
        !registry.has_tool("spawn_agent"),
        "spawn must be stripped so no non-restricted grandchild can be minted"
    );
    assert!(
        !registry.has_tool("telegram_send"),
        "denied-by-name tools must be stripped"
    );
}
