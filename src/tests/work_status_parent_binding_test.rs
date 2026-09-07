//! Parent-binding tests for the boot-resume path (#110).
//!
//! A revived sub-agent session must be recognizable from its status file and
//! the file must carry the spawning session: without that binding the
//! resumed result routes to the surface-less default and vanishes.

use crate::brain::agent::service::work_status::{WorkKind, WorkState, WorkStatus};
use uuid::Uuid;

fn temp_status_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "oc-ws-parent-test-{tag}-{}-{}",
        std::process::id(),
        Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    crate::brain::agent::service::work_status::test_override::set(dir.clone());
    dir
}

fn drop_status_dir(dir: std::path::PathBuf) {
    crate::brain::agent::service::work_status::test_override::clear();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn status_file_carries_parent_and_lookup_finds_child() {
    let dir = temp_status_dir("carry");
    let child = Uuid::new_v4();
    let parent = Uuid::new_v4();

    WorkStatus::new_agent(
        "agent-carry",
        "review lens",
        &child.to_string(),
        "do the review",
        Some(&parent.to_string()),
    )
    .expect("write status");

    // Round-trip: the parent survives the disk write.
    let status = WorkStatus::read("agent-carry").expect("status exists");
    assert_eq!(
        status.parent_session_id.as_deref(),
        Some(parent.to_string().as_str())
    );
    assert_eq!(status.session_id, child.to_string());
    assert!(!status.state.is_terminal());

    // The resume path's detector: found by child session, non-terminal.
    let found = WorkStatus::find_agent_by_session(&child.to_string()).expect("found");
    assert_eq!(found.id, "agent-carry");
    assert_eq!(
        found.parent_session_id.as_deref(),
        Some(parent.to_string().as_str())
    );

    // Unknown session: no hit, no panic.
    assert!(WorkStatus::find_agent_by_session(&Uuid::new_v4().to_string()).is_none());

    drop_status_dir(dir);
}

#[test]
fn interrupted_file_is_still_detected_after_reconcile() {
    let dir = temp_status_dir("interrupted");
    let child = Uuid::new_v4();
    let parent = Uuid::new_v4();

    let mut agent = WorkStatus::new_agent(
        "agent-int",
        "restarted lens",
        &child.to_string(),
        "task",
        Some(&parent.to_string()),
    )
    .expect("write status");

    // Upstream reconciliation marks the file Interrupted at boot, before
    // the resumed session finishes — the detector must still find it.
    agent.mark_interrupted().expect("mark interrupted");
    assert!(agent.state.is_terminal());
    let found = WorkStatus::find_agent_by_session(&child.to_string()).expect("found");
    assert_eq!(found.id, "agent-int");
    assert_eq!(
        found.parent_session_id.as_deref(),
        Some(parent.to_string().as_str())
    );

    drop_status_dir(dir);
}

#[test]
fn legacy_file_without_parent_deserializes_and_is_skipped_by_nothing() {
    let dir = temp_status_dir("legacy");
    let child = Uuid::new_v4();
    let path = crate::brain::agent::service::work_status::status_path("agent-legacy");
    let body = format!(
        r#"{{"id":"agent-legacy","kind":"agent","session_id":"{child}","label":"old","task":"p","spawned_at":"2026-01-01T00:00:00Z","state":"Running"}}"#
    );
    std::fs::write(&path, body).expect("write legacy file");

    let status = WorkStatus::read("agent-legacy").expect("legacy parses");
    assert!(
        status.parent_session_id.is_none(),
        "legacy file has no parent"
    );
    assert_eq!(status.session_id, child.to_string());
    // Still detected as a revived agent (so its result at least finalizes the
    // file), but with no parent to route to.
    assert!(WorkStatus::find_agent_by_session(&child.to_string()).is_some());

    drop_status_dir(dir);
}

#[test]
fn lookup_skips_commands_and_outcome_terminal_agents() {
    let dir = temp_status_dir("skip");
    let child = Uuid::new_v4();

    // A detached command sharing the session id must not look like an agent.
    WorkStatus::new_command("cmd-1", &child.to_string(), "a command", "ls")
        .expect("write command status");
    assert!(WorkStatus::find_agent_by_session(&child.to_string()).is_none());

    // An outcome-terminal agent must not look revivable.
    let mut agent = WorkStatus::new_agent(
        "agent-done",
        "finished lens",
        &child.to_string(),
        "task",
        None,
    )
    .expect("write agent status");
    agent.mark_completed("done".to_string()).expect("finalize");
    assert!(matches!(agent.state, WorkState::Completed));
    assert_eq!(agent.kind, WorkKind::Agent);
    assert!(WorkStatus::find_agent_by_session(&child.to_string()).is_none());

    // A failed agent is equally final.
    let mut agent = WorkStatus::new_agent(
        "agent-fail",
        "failed lens",
        &child.to_string(),
        "task",
        None,
    )
    .expect("write agent status");
    agent.mark_failed("boom".to_string()).expect("finalize");
    assert!(WorkStatus::find_agent_by_session(&child.to_string()).is_none());

    drop_status_dir(dir);
}
