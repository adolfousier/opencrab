//! #1160 tasks_list + detached status files + prompt rot-guard.

use crate::brain::agent::service::detached_status::{self, DetachedFinish, test_override};
use crate::brain::tools::Tool;
use crate::brain::tools::tasks_list::{DetachedRow, SubagentRow, TasksListTool, render_tasks};
use tempfile::TempDir;
use uuid::Uuid;

#[test]
fn schema_has_no_params_and_tool_is_read_only() {
    let tool = TasksListTool::new();
    assert_eq!(tool.name(), "tasks_list");
    assert_eq!(
        tool.input_schema()["properties"].as_object().unwrap().len(),
        0
    );
    assert!(tool.hints().read_only);
}

#[test]
fn render_empty_roster_says_so_explicitly() {
    let out = render_tasks(&[], &[]);
    assert_eq!(out, "No background tasks.");
}

#[test]
fn render_lists_both_systems_with_states_and_pointers() {
    let subs = vec![SubagentRow {
        id: "agt-1".into(),
        label: "research".into(),
        state: "running".into(),
        status_file: Some("/tmp/subagents/agt-1.json".into()),
    }];
    let det = vec![DetachedRow {
        label: "cargo test".into(),
        elapsed_secs: 42,
    }];
    let out = render_tasks(&subs, &det);
    assert!(out.contains("Sub-agents (1)"), "was: {out}");
    assert!(out.contains("- agt-1 [research] running"), "was: {out}");
    assert!(out.contains("status file: /tmp/subagents/agt-1.json"));
    assert!(out.contains("Detached commands (1)"), "was: {out}");
    assert!(out.contains("- cargo test (elapsed 42s)"), "was: {out}");
}

/// Gap 2: a detached command's status file exists mid-run with spawn data,
/// and gains exit info on completion.
#[test]
fn detached_status_file_written_then_finished() {
    let dir = TempDir::new().unwrap();
    test_override::set(dir.path().to_path_buf());

    let task_id = Uuid::new_v4();
    let session_id = Uuid::new_v4();
    detached_status::write_started(task_id, session_id, "cargo test", "cargo test --lib");
    let raw = std::fs::read_to_string(dir.path().join(format!("{task_id}.json"))).unwrap();
    assert!(raw.contains("\"cargo test\""), "was: {raw}");
    assert!(raw.contains(&session_id.to_string()), "was: {raw}");
    assert!(!raw.contains("\"finished\""), "mid-run must be unfinished");

    detached_status::write_finished(
        task_id,
        session_id,
        "cargo test",
        "cargo test --lib",
        DetachedFinish {
            success: true,
            code: 0,
            elapsed_secs: 99.5,
            output_bytes: 2048,
        },
    );
    let raw = std::fs::read_to_string(dir.path().join(format!("{task_id}.json"))).unwrap();
    assert!(raw.contains("\"success\": true"), "was: {raw}");
    assert!(raw.contains("\"output_bytes\": 2048"), "was: {raw}");
}

/// Gap 3 rot-guard: the LONG TASKS paragraph must keep covering sub-agents,
/// not only bash — it regressed to bash-only once already (#762).
#[test]
fn prompt_builder_keeps_subagent_background_contract() {
    let dir = TempDir::new().unwrap();
    let prompt = crate::brain::prompt_builder::BrainLoader::new(dir.path().to_path_buf())
        .build_system_brain(None);
    assert!(
        prompt.contains("spawned agents run in the background"),
        "subagent background contract missing from system prompt"
    );
}
