//! Dedicated coverage for the plan-mode tool gate
//! (`brain::tools::plan_gate`), driven by MCP-style [`ToolHints`].
//! Verifies, per state:
//!
//! - pre-init Editing allows read-only tools, sends mutators to
//!   RequireApproval
//! - post-init Editing sends mutators to RequireApproval, allows
//!   writes ONLY to the session `.md`
//! - Active freezes the `.md` against generic write tools; the seed
//!   window hard-denies mutators
//! - NoPlan gates nothing
//!
//! The gate is pure: `hints.read_only` is the sole signal. No name
//! lists, no capability branching.

use crate::brain::tools::plan_gate::check_plan_gate;
use crate::brain::tools::r#trait::ToolHints;
use crate::config::profile::{home_for_profile, with_profile_home_async};
use crate::tui::plan::{PlanDocument, PlanStatus, PlanTask, TaskType};
use crate::utils::plan_files::{create_design_md, plan_md_path, save_plan, set_pre_init_editing};
use serde_json::json;
use uuid::Uuid;

/// Read-only hint (read_file, grep, web_search, plan, etc.).
fn ro() -> ToolHints {
    ToolHints {
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: false,
    }
}

/// Mutator hint (bash, edit_file, telegram_send, spawn_agent, etc.).
fn mut_tool() -> ToolHints {
    ToolHints {
        read_only: false,
        destructive: true,
        idempotent: false,
        open_world: true,
    }
}

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
        for (name, hints) in [
            ("edit_file", &mut_tool()),
            ("bash", &mut_tool()),
            ("telegram_send", &mut_tool()),
            ("spawn_agent", &mut_tool()),
        ] {
            assert!(
                check_plan_gate(sid, name, hints, &json!({}))
                    .await
                    .is_allowed(),
                "{name} must pass with no plan"
            );
        }
    })
    .await;
}

#[tokio::test]
async fn pre_init_gates_mutators_allows_reads_and_plan() {
    in_temp_home(async {
        let sid = Uuid::new_v4();
        set_pre_init_editing(sid).await.unwrap();

        // Mutators go to RequireApproval in pre-init Editing.
        for (name, hints) in [
            ("edit_file", &mut_tool()),
            ("bash", &mut_tool()),
            ("execute_code", &mut_tool()),
            ("telegram_send", &mut_tool()),
            ("browser_click", &mut_tool()),
            ("spawn_agent", &mut_tool()),
            ("evolve", &mut_tool()),
        ] {
            assert!(
                check_plan_gate(sid, name, hints, &json!({}))
                    .await
                    .needs_approval(),
                "{name} must require approval pre-init"
            );
        }

        // Brain-file writes are mutators too.
        let deny = check_plan_gate(
            sid,
            "write_opencrabs_file",
            &mut_tool(),
            &json!({"path": "MEMORY.md"}),
        )
        .await;
        assert!(
            deny.needs_approval(),
            "opencrabs write must require approval pre-init"
        );

        // Read-only tools flow through.
        for (name, hints) in [("read_file", &ro()), ("web_search", &ro()), ("plan", &ro())] {
            assert!(
                check_plan_gate(sid, name, hints, &json!({}))
                    .await
                    .is_allowed(),
                "{name} must be allowed pre-init"
            );
        }
    })
    .await;
}

#[tokio::test]
async fn post_init_gates_mutators_to_md_only() {
    in_temp_home(async {
        let sid = Uuid::new_v4();
        make_post_init_editing(sid).await;
        let md = plan_md_path(sid).await;

        // Mutators go to approval (RequireApproval), not hard deny.
        for (name, hints) in [
            ("bash", &mut_tool()),
            ("execute_code", &mut_tool()),
            ("slack_send", &mut_tool()),
            ("browser_eval", &mut_tool()),
            ("resume_agent", &mut_tool()),
            ("cron_manage", &mut_tool()),
        ] {
            assert!(
                check_plan_gate(sid, name, hints, &json!({}))
                    .await
                    .needs_approval(),
                "{name} must require approval post-init"
            );
        }

        // The session .md is the only writable file for a mutator.
        let ok = check_plan_gate(
            sid,
            "write_file",
            &mut_tool(),
            &json!({"path": md.to_string_lossy()}),
        )
        .await;
        assert!(ok.is_allowed(), "session .md write must pass, got: {ok:?}");

        // edit_file on the .md passes too (path key is shared).
        assert!(
            check_plan_gate(
                sid,
                "edit_file",
                &mut_tool(),
                &json!({"path": md.to_string_lossy()})
            )
            .await
            .is_allowed()
        );

        // Project writes go to approval.
        assert!(
            check_plan_gate(
                sid,
                "edit_file",
                &mut_tool(),
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
                &mut_tool(),
                &json!({"path": "MEMORY.md"})
            )
            .await
            .needs_approval()
        );

        // A mutator with no recognizable target goes to approval.
        assert!(
            check_plan_gate(sid, "generate_document", &mut_tool(), &json!({}))
                .await
                .needs_approval()
        );

        // Read-only tools flow through.
        for (name, hints) in [("read_file", &ro()), ("grep", &ro()), ("plan", &ro())] {
            assert!(
                check_plan_gate(sid, name, hints, &json!({}))
                    .await
                    .is_allowed(),
                "{name} must be allowed post-init"
            );
        }
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
                &mut_tool(),
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
                &mut_tool(),
                &json!({"path": "/tmp/project/main.rs"})
            )
            .await
            .is_allowed()
        );
        assert!(
            check_plan_gate(sid, "bash", &mut_tool(), &json!({"command": "ls"}))
                .await
                .is_allowed()
        );
        assert!(
            check_plan_gate(sid, "telegram_send", &mut_tool(), &json!({}))
                .await
                .is_allowed()
        );
        assert!(
            check_plan_gate(sid, "spawn_agent", &mut_tool(), &json!({}))
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
                &mut_tool(),
                &json!({"path": "/tmp/project/main.rs"})
            )
            .await
            .is_allowed()
        );
        assert!(
            check_plan_gate(sid, "bash", &mut_tool(), &json!({"command": "ls"}))
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
        // tasks): only read-only tools and the plan tool flow through.
        let sid = Uuid::new_v4();
        let mut plan = PlanDocument::new(sid, "Seeding".to_string());
        plan.status = PlanStatus::Active;
        save_plan(&plan).await.unwrap();
        create_design_md(sid, "Seeding").await.unwrap();

        assert!(
            check_plan_gate(sid, "plan", &ro(), &json!({"operation": "add_tasks"}))
                .await
                .is_allowed()
        );
        assert!(
            check_plan_gate(sid, "read_file", &ro(), &json!({}))
                .await
                .is_allowed()
        );
        // Mutators are hard-denied in the seed window.
        assert!(
            check_plan_gate(sid, "bash", &mut_tool(), &json!({"command": "ls"}))
                .await
                .is_denied()
        );
        assert!(
            check_plan_gate(
                sid,
                "edit_file",
                &mut_tool(),
                &json!({"path": "/tmp/p/main.rs"})
            )
            .await
            .is_denied()
        );
        assert!(
            check_plan_gate(sid, "spawn_agent", &mut_tool(), &json!({}))
                .await
                .is_denied(),
            "spawn must be blocked in seed window (mutator)"
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
            check_plan_gate(sid, "bash", &mut_tool(), &json!({"command": "ls"}))
                .await
                .is_denied()
        );

        // Once a task starts, the seed window closes and normal Active
        // policy applies (only the .md stays frozen).
        let mut plan = crate::utils::plan_files::load_plan(sid).await.unwrap();
        plan.tasks[0].start();
        save_plan(&plan).await.unwrap();
        assert!(
            check_plan_gate(sid, "bash", &mut_tool(), &json!({"command": "ls"}))
                .await
                .is_allowed()
        );
        assert!(
            check_plan_gate(
                sid,
                "edit_file",
                &mut_tool(),
                &json!({"path": "/tmp/p/main.rs"})
            )
            .await
            .is_allowed()
        );
    })
    .await;
}

// ── restrict_registry_to_read_only (#649) ───────────────────────────────
// A subagent spawned while the parent is Editing must be read-only. The
// filter strips non-read-only tools from the child registry via the MCP
// `read_only` hint (child runs under a fresh NoPlan session the per-call
// gate would not catch), keeping only read/search/network tools.

struct CapTool {
    name: &'static str,
    caps: Vec<crate::brain::tools::r#trait::ToolCapability>,
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
    fn capabilities(&self) -> Vec<crate::brain::tools::r#trait::ToolCapability> {
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
    use crate::brain::tools::r#trait::ToolCapability;
    use std::sync::Arc;

    let registry = ToolRegistry::new();
    // Read-only surface (no destructive caps) must survive.
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
        name: "suggest_followups",
        caps: vec![],
    }));
    // Mutators (destructive trio) must be stripped by the read_only hint.
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

    restrict_registry_to_read_only(&registry);

    assert!(registry.has_tool("read_file"), "reads must survive");
    assert!(registry.has_tool("grep"), "search must survive");
    assert!(
        registry.has_tool("http_request"),
        "network read must survive"
    );
    assert!(
        registry.has_tool("suggest_followups"),
        "the suggestion tool must survive"
    );
    assert!(!registry.has_tool("edit_file"), "writes must be stripped");
    assert!(!registry.has_tool("bash"), "bash must be stripped");
    assert!(
        !registry.has_tool("spawn_agent"),
        "spawn must be stripped so no non-restricted grandchild can be minted"
    );
}
