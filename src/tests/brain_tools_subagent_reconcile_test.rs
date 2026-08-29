//! Startup reconciliation of orphaned detached-work status files
//! (#1038, unified #26).
//!
//! A sub-agent or detached command dies with the process; its status file
//! does not. Every file still `Pending` or `Running` at startup belongs to
//! work that no longer exists, and must stop reading as live.

use crate::brain::agent::service::work_status::*;
use crate::brain::tools::subagent::reconcile::reconcile_orphaned_agents;
use std::fs;

/// Point the test override at a NESTED dir so [`legacy_dir`] (the
/// `subagents` sibling the reconcile pass migrates from) resolves inside
/// the per-test sandbox instead of the real /tmp.
fn isolate(tag: &str) {
    let dir = std::env::temp_dir().join(format!(
        "opencrabs-subagent-reconcile-{}-{}",
        tag,
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    test_override::set(dir.join("detached"));
}

#[test]
fn a_running_agent_becomes_interrupted() {
    isolate("running");
    let mut s = WorkStatus::new_agent("agent-1", "build docs", "sess-a", "do things").unwrap();
    s.mark_running().unwrap();

    let orphans = reconcile_orphaned_agents();

    assert_eq!(orphans.len(), 1);
    assert_eq!(orphans[0].id, "agent-1");
    assert_eq!(orphans[0].state, WorkState::Interrupted);

    // Persisted, not just returned: the next reader must see it too.
    let reread = WorkStatus::read("agent-1").expect("status file still present");
    assert_eq!(reread.state, WorkState::Interrupted);
}

#[test]
fn a_pending_agent_becomes_interrupted() {
    // Killed between the status file being written and the task starting.
    isolate("pending");
    WorkStatus::new_agent("agent-2", "lint", "sess-b", "do things").unwrap();

    let orphans = reconcile_orphaned_agents();

    assert_eq!(orphans.len(), 1);
    assert_eq!(orphans[0].state, WorkState::Interrupted);
}

#[test]
fn a_parked_agent_becomes_interrupted() {
    // #1183: a file parked `AwaitingInput` is non-terminal, so a restart must
    // interrupt it like any other live agent — before the parked state
    // existed such a file read `Running` and was swept by the same rule, and
    // the new variant must not slip through it.
    isolate("parked");
    let mut s = WorkStatus::new_agent("agent-4", "audit", "sess-d", "do things").unwrap();
    s.mark_running().unwrap();
    s.mark_awaiting_input().unwrap();

    let orphans = reconcile_orphaned_agents();

    assert_eq!(orphans.len(), 1);
    assert_eq!(orphans[0].state, WorkState::Interrupted);
    let reread = WorkStatus::read("agent-4").expect("status file still present");
    assert_eq!(reread.state, WorkState::Interrupted);
}

#[test]
fn interrupted_carries_a_reason_and_a_completion_stamp() {
    // The reason distinguishes a restart from a genuine failure, and the
    // stamp lets the file age out on the same schedule as any other
    // terminal state.
    isolate("reason");
    let mut s = WorkStatus::new_agent("agent-3", "test", "sess-c", "do things").unwrap();
    s.mark_running().unwrap();

    let orphans = reconcile_orphaned_agents();

    let finish = orphans[0]
        .finish
        .as_ref()
        .expect("interrupted stamps a finish");
    assert!(
        finish.error.as_deref().unwrap_or("").contains("restart"),
        "was: {:?}",
        finish.error
    );
    assert!(!finish.completed_at.is_empty());
}

#[test]
fn terminal_agents_are_left_alone() {
    isolate("terminal");
    let mut done = WorkStatus::new_agent("agent-done", "a", "sess-d", "p").unwrap();
    done.mark_completed("output".to_string()).unwrap();
    let mut failed = WorkStatus::new_agent("agent-failed", "b", "sess-d", "p").unwrap();
    failed.mark_failed("boom".to_string()).unwrap();

    let orphans = reconcile_orphaned_agents();

    assert!(orphans.is_empty(), "nothing mid-flight to reconcile");
    assert_eq!(
        WorkStatus::read("agent-done").unwrap().state,
        WorkState::Completed
    );
    assert_eq!(
        WorkStatus::read("agent-failed").unwrap().state,
        WorkState::Failed
    );
}

#[test]
fn the_parent_session_survives_so_the_report_can_be_routed() {
    // The whole point of returning the statuses: the caller needs to know
    // which session to tell.
    isolate("parent");
    let mut s = WorkStatus::new_agent("agent-4", "deploy", "sess-parent", "p").unwrap();
    s.mark_running().unwrap();

    let orphans = reconcile_orphaned_agents();

    assert_eq!(orphans[0].session_id, "sess-parent");
}

#[test]
fn reconciling_is_idempotent() {
    // A second startup must not re-report an agent already accounted for.
    isolate("idempotent");
    let mut s = WorkStatus::new_agent("agent-5", "x", "sess-e", "p").unwrap();
    s.mark_running().unwrap();

    assert_eq!(reconcile_orphaned_agents().len(), 1);
    assert!(reconcile_orphaned_agents().is_empty());
}

#[test]
fn a_missing_status_dir_is_not_an_error() {
    isolate("missing");
    assert!(reconcile_orphaned_agents().is_empty());
}

#[test]
fn unparseable_files_do_not_abort_the_pass() {
    isolate("corrupt");
    ensure_dir().unwrap();
    fs::write(status_dir().join("garbage.json"), "not json at all").unwrap();
    let mut s = WorkStatus::new_agent("agent-6", "y", "sess-f", "p").unwrap();
    s.mark_running().unwrap();

    let orphans = reconcile_orphaned_agents();

    assert_eq!(orphans.len(), 1, "the healthy file is still reconciled");
    assert_eq!(orphans[0].id, "agent-6");
}

// ── #26 P1: two kinds share the dir ─────────────────────────────────

#[test]
fn a_running_command_is_interrupted_on_disk_but_not_reported() {
    // Commands share the dir since #26, so the pass must stop them reading
    // as live too — but their interruption report still rides the DB row
    // (#763): returning them here as well would double-message until the
    // boot pass unifies (P2).
    isolate("command");
    WorkStatus::new_command("cmd-1", "sess-g", "nightly build", "cargo build").unwrap();

    let orphans = reconcile_orphaned_agents();

    assert!(
        orphans.is_empty(),
        "commands are not returned for reporting"
    );
    let reread = WorkStatus::read("cmd-1").expect("command file still present");
    assert_eq!(reread.state, WorkState::Interrupted, "marked on disk");
    assert!(
        reread
            .finish
            .as_ref()
            .and_then(|f| f.error.as_deref())
            .unwrap_or("")
            .contains("this command"),
        "kind-aware interruption text"
    );
}

#[test]
fn mixed_kinds_report_only_the_agents() {
    isolate("mixed");
    let mut agent = WorkStatus::new_agent("agent-m", "docs", "sess-m", "p").unwrap();
    agent.mark_running().unwrap();
    WorkStatus::new_command("cmd-m", "sess-m", "build", "make").unwrap();

    let orphans = reconcile_orphaned_agents();

    assert_eq!(orphans.len(), 1);
    assert_eq!(orphans[0].id, "agent-m");
    assert_eq!(orphans[0].kind, WorkKind::Agent);
}

#[test]
fn a_legacy_running_agent_is_migrated_then_reported() {
    // End-to-end upgrade path (#1038 survives the dir move): a pre-#26
    // agent file in the old dir is migrated into the unified dir first,
    // then interrupted and returned like any other orphan.
    isolate("legacy_e2e");
    fs::create_dir_all(legacy_dir()).unwrap();
    let legacy = serde_json::json!({
        "id": "agent-old",
        "label": "old worker",
        "parent_session_id": "sess-old",
        "state": "Running",
        "prompt": "do old things",
        "started_at": "2026-08-28T10:00:00+00:00"
    });
    fs::write(
        legacy_dir().join("agent-old.json"),
        serde_json::to_string_pretty(&legacy).unwrap(),
    )
    .unwrap();

    let orphans = reconcile_orphaned_agents();

    assert_eq!(orphans.len(), 1);
    assert_eq!(orphans[0].id, "agent-old");
    assert_eq!(orphans[0].kind, WorkKind::Agent);
    assert_eq!(
        orphans[0].session_id, "sess-old",
        "routing survives migration"
    );
    assert_eq!(orphans[0].state, WorkState::Interrupted);
    assert!(!legacy_dir().exists(), "legacy dir consumed");
    assert!(status_path("agent-old").exists(), "file moved, not lost");
}
