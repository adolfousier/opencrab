use crate::brain::tools::subagent::status::*;
use std::fs;
use std::time::Duration;

fn isolate(tag: &str) {
    let dir = std::env::temp_dir().join(format!(
        "opencrabs-subagent-test-{}-{}",
        tag,
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    test_override::set(dir);
}

#[test]
fn status_dir_returns_correct_path() {
    let home = crate::config::opencrabs_home();
    let expected = home.join("tmp").join("subagents");
    assert_eq!(status_dir(), expected);
}

#[test]
fn status_path_ends_with_json() {
    let p = status_path("abc123");
    assert_eq!(p.file_name().unwrap().to_str().unwrap(), "abc123.json");
}

#[test]
fn new_status_is_pending() {
    isolate("new_pending");
    let s = AgentStatus::new("test-1", "test", "sess-1", "do things").unwrap();
    assert_eq!(s.state, AgentState::Pending);
    assert_eq!(s.id, "test-1");
    assert_eq!(s.label, "test");
}

#[test]
fn status_transitions_to_running() {
    isolate("running");
    let mut s = AgentStatus::new("test-2", "test", "sess-1", "do things").unwrap();
    s.mark_running().unwrap();
    assert_eq!(s.state, AgentState::Running);
}

#[test]
fn status_progress_snapshot() {
    isolate("progress");
    let mut s = AgentStatus::new("test-3", "test", "sess-1", "do things").unwrap();
    s.mark_running().unwrap();
    s.update_progress(1, Some("bash".into()), Some("cargo check ok".into()))
        .unwrap();
    assert!(s.progress.is_some());
    let p = s.progress.unwrap();
    assert_eq!(p.iteration, 1);
    assert_eq!(p.last_tool, Some("bash".to_string()));
    assert_eq!(p.last_event, Some("cargo check ok".to_string()));
}

#[test]
fn status_completed_sets_timestamp() {
    isolate("completed");
    let mut s = AgentStatus::new("test-4", "test", "sess-1", "do things").unwrap();
    s.mark_completed("done".into()).unwrap();
    assert_eq!(s.state, AgentState::Completed);
    assert!(s.completed_at.is_some());
    assert_eq!(s.output_summary, Some("done".to_string()));
}

#[test]
fn status_failed_sets_error() {
    isolate("failed");
    let mut s = AgentStatus::new("test-5", "test", "sess-1", "do things").unwrap();
    s.mark_failed("something broke".into()).unwrap();
    assert_eq!(s.state, AgentState::Failed);
    assert_eq!(s.error, Some("something broke".to_string()));
    assert!(s.completed_at.is_some());
}

#[test]
fn status_read_roundtrip() {
    isolate("roundtrip");
    let mut s = AgentStatus::new("test-6", "test", "sess-1", "do things").unwrap();
    s.mark_running().unwrap();
    s.update_progress(2, Some("write_file".into()), None)
        .unwrap();

    let read = AgentStatus::read("test-6").expect("should read back");
    assert_eq!(read.id, "test-6");
    assert_eq!(read.state, AgentState::Running);
    assert_eq!(read.progress.unwrap().iteration, 2);
}

#[test]
fn cleanup_removes_old_files() {
    isolate("cleanup");
    let mut s = AgentStatus::new("old-1", "old", "sess", "task").unwrap();
    s.mark_completed("done".into()).unwrap();

    let old_ts = chrono::Utc::now()
        .checked_sub_signed(chrono::Duration::days(8))
        .unwrap()
        .to_rfc3339();
    let mut raw = fs::read_to_string(status_path("old-1")).unwrap();
    let mut parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
    parsed["completed_at"] = serde_json::json!(old_ts);
    raw = serde_json::to_string_pretty(&parsed).unwrap();
    fs::write(status_path("old-1"), raw).unwrap();

    let cleanup_result = cleanup_stale(Duration::from_secs(7 * 86400)).unwrap();
    assert!(cleanup_result.1 >= 1, "should have removed at least 1 file");
    assert!(AgentStatus::read("old-1").is_none());
}
