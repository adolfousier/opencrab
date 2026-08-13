//! Startup reconciliation of orphaned sub-agent status files (#1038).
//!
//! A sub-agent dies with the process; its status file does not. Every file
//! still `Pending` or `Running` at startup belongs to an agent that no longer
//! exists, and must stop reading as live.

use crate::brain::tools::subagent::reconcile::reconcile_orphaned_agents;
use crate::brain::tools::subagent::status::*;
use std::fs;

fn isolate(tag: &str) {
    let dir = std::env::temp_dir().join(format!(
        "opencrabs-subagent-reconcile-{}-{}",
        tag,
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    test_override::set(dir);
}

#[test]
fn a_running_agent_becomes_interrupted() {
    isolate("running");
    let mut s = AgentStatus::new("agent-1", "build docs", "sess-a", "do things").unwrap();
    s.mark_running().unwrap();

    let orphans = reconcile_orphaned_agents();

    assert_eq!(orphans.len(), 1);
    assert_eq!(orphans[0].id, "agent-1");
    assert_eq!(orphans[0].state, AgentState::Interrupted);

    // Persisted, not just returned: the next reader must see it too.
    let reread = AgentStatus::read("agent-1").expect("status file still present");
    assert_eq!(reread.state, AgentState::Interrupted);
}

#[test]
fn a_pending_agent_becomes_interrupted() {
    // Killed between the status file being written and the task starting.
    isolate("pending");
    AgentStatus::new("agent-2", "lint", "sess-b", "do things").unwrap();

    let orphans = reconcile_orphaned_agents();

    assert_eq!(orphans.len(), 1);
    assert_eq!(orphans[0].state, AgentState::Interrupted);
}

#[test]
fn interrupted_carries_a_reason_and_a_completion_stamp() {
    // The reason distinguishes a restart from a genuine failure, and the
    // stamp lets the file age out on the same schedule as any other
    // terminal state.
    isolate("reason");
    let mut s = AgentStatus::new("agent-3", "test", "sess-c", "do things").unwrap();
    s.mark_running().unwrap();

    let orphans = reconcile_orphaned_agents();

    assert!(
        orphans[0]
            .error
            .as_deref()
            .unwrap_or("")
            .contains("restart")
    );
    assert!(orphans[0].completed_at.is_some());
}

#[test]
fn terminal_agents_are_left_alone() {
    isolate("terminal");
    let mut done = AgentStatus::new("agent-done", "a", "sess-d", "p").unwrap();
    done.mark_completed("output".to_string()).unwrap();
    let mut failed = AgentStatus::new("agent-failed", "b", "sess-d", "p").unwrap();
    failed.mark_failed("boom".to_string()).unwrap();

    let orphans = reconcile_orphaned_agents();

    assert!(orphans.is_empty(), "nothing mid-flight to reconcile");
    assert_eq!(
        AgentStatus::read("agent-done").unwrap().state,
        AgentState::Completed
    );
    assert_eq!(
        AgentStatus::read("agent-failed").unwrap().state,
        AgentState::Failed
    );
}

#[test]
fn the_parent_session_survives_so_the_report_can_be_routed() {
    // The whole point of returning the statuses: the caller needs to know
    // which session to tell.
    isolate("parent");
    let mut s = AgentStatus::new("agent-4", "deploy", "sess-parent", "p").unwrap();
    s.mark_running().unwrap();

    let orphans = reconcile_orphaned_agents();

    assert_eq!(orphans[0].parent_session_id, "sess-parent");
}

#[test]
fn reconciling_is_idempotent() {
    // A second startup must not re-report an agent already accounted for.
    isolate("idempotent");
    let mut s = AgentStatus::new("agent-5", "x", "sess-e", "p").unwrap();
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
    let mut s = AgentStatus::new("agent-6", "y", "sess-f", "p").unwrap();
    s.mark_running().unwrap();

    let orphans = reconcile_orphaned_agents();

    assert_eq!(orphans.len(), 1, "the healthy file is still reconciled");
    assert_eq!(orphans[0].id, "agent-6");
}
