//! Tests for the RSI proposals inbox + the propose / proposals tools.
//!
//! Storage tests use `ProposalsStore::with_dir(tmpdir)` so they never
//! touch the user's real `~/.opencrabs/rsi/` inbox. Tool-level tests
//! exercise `rsi_propose` against the real `opencrabs_home()` resolver
//! is intentionally avoided — we only run the parts that don't write to
//! disk (input validation, error paths) without test isolation. The
//! `apply` round-trip test uses a tmpdir for both the inbox and the
//! `tools.toml` install target.

use crate::brain::commands::{CommandLoader, UserCommand};
use crate::brain::rsi_proposals::ProposalsStore;
use crate::brain::tools::dynamic::DynamicToolLoader;
use crate::brain::tools::dynamic::tool::{DynamicToolDef, ExecutorType};
use crate::brain::tools::rsi_proposals::RsiProposalsTool;
use crate::brain::tools::{Tool, ToolExecutionContext, ToolRegistry};
use std::sync::Arc;
use tempfile::TempDir;
use uuid::Uuid;

fn ctx() -> ToolExecutionContext {
    ToolExecutionContext::new(Uuid::new_v4())
}

fn sample_tool_def() -> DynamicToolDef {
    DynamicToolDef {
        name: "gh_issue_list".to_string(),
        description: "List GitHub issues".to_string(),
        executor: ExecutorType::Shell,
        enabled: true,
        requires_approval: true,
        method: None,
        url: None,
        headers: Default::default(),
        timeout_secs: 30,
        command: Some("gh issue list".to_string()),
        params: Vec::new(),
    }
}

fn sample_command() -> UserCommand {
    UserCommand {
        name: "/deploy".to_string(),
        description: "Deploy to production".to_string(),
        action: "prompt".to_string(),
        prompt: "Run the deploy script and report status".to_string(),
    }
}

#[test]
fn storage_add_and_list_tool_proposal() {
    let dir = TempDir::new().unwrap();
    let store = ProposalsStore::with_dir(dir.path().to_path_buf());

    let id = store
        .add_tool_proposal(
            "rsi-autonomous",
            "obs: 12 manual gh calls",
            sample_tool_def(),
        )
        .unwrap();
    assert!(id.starts_with("prop_tool_"));

    let proposals = store.list_tool_proposals();
    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0].id, id);
    assert_eq!(proposals[0].def.name, "gh_issue_list");
    assert_eq!(proposals[0].rationale, "obs: 12 manual gh calls");
}

#[test]
fn storage_add_and_list_command_proposal() {
    let dir = TempDir::new().unwrap();
    let store = ProposalsStore::with_dir(dir.path().to_path_buf());

    let id = store
        .add_command_proposal(
            "rsi-autonomous",
            "user typed /deploy 5 times",
            sample_command(),
        )
        .unwrap();
    assert!(id.starts_with("prop_cmd_"));

    let proposals = store.list_command_proposals();
    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0].command.name, "/deploy");
}

#[test]
fn storage_dedup_by_name() {
    // A second proposal for the same tool name supersedes the first.
    let dir = TempDir::new().unwrap();
    let store = ProposalsStore::with_dir(dir.path().to_path_buf());

    store
        .add_tool_proposal("rsi-autonomous", "first cycle", sample_tool_def())
        .unwrap();
    let id2 = store
        .add_tool_proposal(
            "rsi-autonomous",
            "second cycle (refined)",
            sample_tool_def(),
        )
        .unwrap();

    let proposals = store.list_tool_proposals();
    assert_eq!(proposals.len(), 1, "older proposal should be replaced");
    assert_eq!(proposals[0].id, id2);
    assert_eq!(proposals[0].rationale, "second cycle (refined)");
}

#[test]
fn storage_take_and_pending_count() {
    let dir = TempDir::new().unwrap();
    let store = ProposalsStore::with_dir(dir.path().to_path_buf());

    let tool_id = store
        .add_tool_proposal("rsi-autonomous", "evidence", sample_tool_def())
        .unwrap();
    let cmd_id = store
        .add_command_proposal("rsi-autonomous", "evidence", sample_command())
        .unwrap();

    assert_eq!(store.pending_count(), 2);

    let taken = store.take_tool_proposal(&tool_id).unwrap();
    assert!(taken.is_some());
    assert_eq!(store.pending_count(), 1);

    // Take of unknown id returns None, doesn't error.
    let missing = store
        .take_tool_proposal("prop_tool_does_not_exist")
        .unwrap();
    assert!(missing.is_none());
    assert_eq!(store.pending_count(), 1);

    let taken_cmd = store.take_command_proposal(&cmd_id).unwrap();
    assert!(taken_cmd.is_some());
    assert_eq!(store.pending_count(), 0);
}

#[test]
fn storage_archive_applied_and_rejected() {
    let dir = TempDir::new().unwrap();
    let store = ProposalsStore::with_dir(dir.path().to_path_buf());

    let id = store
        .add_tool_proposal("rsi-autonomous", "ev", sample_tool_def())
        .unwrap();
    let proposal = store.take_tool_proposal(&id).unwrap().unwrap();
    store.archive_applied_tool(&proposal).unwrap();

    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let applied_path = dir
        .path()
        .join("applied")
        .join(format!("{}-tools.toml", date));
    assert!(applied_path.exists(), "applied archive must exist");
    let body = std::fs::read_to_string(&applied_path).unwrap();
    assert!(body.contains("gh_issue_list"));

    // Reject path (with reason)
    let id2 = store
        .add_tool_proposal("rsi-autonomous", "ev2", sample_tool_def())
        .unwrap();
    let proposal2 = store.take_tool_proposal(&id2).unwrap().unwrap();
    store
        .archive_rejected_tool(&proposal2, Some("not safe enough"))
        .unwrap();
    let rejected_path = dir
        .path()
        .join("rejected")
        .join(format!("{}-tools.toml", date));
    assert!(rejected_path.exists());
    let rbody = std::fs::read_to_string(&rejected_path).unwrap();
    assert!(rbody.contains("not safe enough"));
}

// ─────────────── rsi_proposals tool — apply round-trip ───────────────

/// Build an RsiProposalsTool wired against tmpdirs for both the
/// inbox (rsi/) and the live tools.toml. brain_path doubles as the
/// commands.toml parent directory. Returns the store handle so tests
/// can pre-load proposals.
fn build_apply_harness() -> (TempDir, RsiProposalsTool, Arc<ToolRegistry>) {
    let dir = TempDir::new().unwrap();
    let brain_path = dir.path().to_path_buf();
    let tools_path = brain_path.join("tools.toml");
    let registry = Arc::new(ToolRegistry::new());

    // Pre-create the rsi/ subdir so ProposalsStore writes land cleanly.
    std::fs::create_dir_all(brain_path.join("rsi")).unwrap();

    let tool = RsiProposalsTool::new(registry.clone(), tools_path, brain_path);
    (dir, tool, registry)
}

#[tokio::test]
async fn list_action_returns_pending_proposals() {
    let (dir, tool, _reg) = build_apply_harness();
    let store = ProposalsStore::with_dir(dir.path().join("rsi"));
    store
        .add_tool_proposal("rsi-autonomous", "manual gh calls", sample_tool_def())
        .unwrap();

    let result = tool
        .execute(serde_json::json!({"action": "list"}), &ctx())
        .await
        .unwrap();
    assert!(result.success);
    let content = &result.output;
    assert!(content.contains("Pending tool proposals"));
    assert!(content.contains("gh_issue_list"));
}

#[tokio::test]
async fn list_returns_empty_message_when_inbox_empty() {
    let (_dir, tool, _reg) = build_apply_harness();
    let result = tool
        .execute(serde_json::json!({"action": "list"}), &ctx())
        .await
        .unwrap();
    assert!(result.success);
    let content = &result.output;
    assert!(content.contains("No pending proposals"));
}

#[tokio::test]
async fn apply_installs_tool_into_live_tools_toml_and_archives() {
    let (dir, tool, registry) = build_apply_harness();
    let store = ProposalsStore::with_dir(dir.path().join("rsi"));
    let id = store
        .add_tool_proposal("rsi-autonomous", "evidence", sample_tool_def())
        .unwrap();

    let result = tool
        .execute(serde_json::json!({"action": "apply", "id": id}), &ctx())
        .await
        .unwrap();
    assert!(
        result.success,
        "apply should succeed: out={} err={:?}",
        result.output, result.error
    );

    // Inbox now empty.
    assert_eq!(store.pending_count(), 0);

    // Tool is registered and listed in tools.toml.
    let tools_path = dir.path().join("tools.toml");
    assert!(tools_path.exists());
    let listed = DynamicToolLoader::list_tools_detailed(&tools_path);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "gh_issue_list");
    assert!(registry.get("gh_issue_list").is_some());

    // Archived under applied/.
    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let applied = dir
        .path()
        .join("rsi")
        .join("applied")
        .join(format!("{}-tools.toml", date));
    assert!(applied.exists());
}

#[tokio::test]
async fn apply_installs_command_into_live_commands_toml() {
    let (dir, tool, _reg) = build_apply_harness();
    let store = ProposalsStore::with_dir(dir.path().join("rsi"));
    let id = store
        .add_command_proposal("rsi-autonomous", "evidence", sample_command())
        .unwrap();

    let result = tool
        .execute(serde_json::json!({"action": "apply", "id": id}), &ctx())
        .await
        .unwrap();
    assert!(
        result.success,
        "out={} err={:?}",
        result.output, result.error
    );

    let loader = CommandLoader::from_brain_path(dir.path());
    let commands = loader.load();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].name, "/deploy");
}

#[tokio::test]
async fn apply_unknown_id_errors_cleanly() {
    let (_dir, tool, _reg) = build_apply_harness();
    let result = tool
        .execute(
            serde_json::json!({"action": "apply", "id": "prop_tool_2026-01-01_xxx_abc123"}),
            &ctx(),
        )
        .await
        .unwrap();
    assert!(!result.success);
    let msg = result.error.clone().unwrap_or_default();
    assert!(
        msg.to_lowercase().contains("no tool proposal"),
        "expected 'no tool proposal' in error, got: {msg:?}"
    );
}

#[tokio::test]
async fn reject_archives_without_installing() {
    let (dir, tool, registry) = build_apply_harness();
    let store = ProposalsStore::with_dir(dir.path().join("rsi"));
    let id = store
        .add_tool_proposal("rsi-autonomous", "ev", sample_tool_def())
        .unwrap();

    let result = tool
        .execute(
            serde_json::json!({
                "action": "reject",
                "id": id,
                "reason": "not safe"
            }),
            &ctx(),
        )
        .await
        .unwrap();
    assert!(result.success);

    // Inbox empty, no install in tools.toml or registry.
    assert_eq!(store.pending_count(), 0);
    assert!(registry.get("gh_issue_list").is_none());
    let tools_path = dir.path().join("tools.toml");
    if tools_path.exists() {
        let listed = DynamicToolLoader::list_tools_detailed(&tools_path);
        assert_eq!(listed.len(), 0);
    }

    // Rejection archived with reason.
    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let rejected = dir
        .path()
        .join("rsi")
        .join("rejected")
        .join(format!("{}-tools.toml", date));
    assert!(rejected.exists());
    let body = std::fs::read_to_string(&rejected).unwrap();
    assert!(body.contains("not safe"));
}

#[tokio::test]
async fn apply_all_installs_every_pending_proposal() {
    let (dir, tool, registry) = build_apply_harness();
    let store = ProposalsStore::with_dir(dir.path().join("rsi"));

    let mut def_b = sample_tool_def();
    def_b.name = "gh_pr_list".to_string();
    def_b.command = Some("gh pr list".to_string());

    store
        .add_tool_proposal("rsi-autonomous", "ev1", sample_tool_def())
        .unwrap();
    store
        .add_tool_proposal("rsi-autonomous", "ev2", def_b)
        .unwrap();
    store
        .add_command_proposal("rsi-autonomous", "ev3", sample_command())
        .unwrap();
    assert_eq!(store.pending_count(), 3);

    let result = tool
        .execute(serde_json::json!({"action": "apply", "id": "all"}), &ctx())
        .await
        .unwrap();
    assert!(result.success);
    assert!(result.output.contains("Applied 3 proposal(s)"));

    assert_eq!(store.pending_count(), 0);
    assert!(registry.get("gh_issue_list").is_some());
    assert!(registry.get("gh_pr_list").is_some());

    let loader = CommandLoader::from_brain_path(dir.path());
    assert_eq!(loader.load().len(), 1);
}

#[tokio::test]
async fn unknown_action_errors() {
    let (_dir, tool, _reg) = build_apply_harness();
    let result = tool
        .execute(serde_json::json!({"action": "delete_everything"}), &ctx())
        .await
        .unwrap();
    assert!(!result.success);
}

// ─────────────── rsi_propose tool — efficiency gate ───────────────

#[tokio::test]
async fn rsi_propose_tool_rejects_without_efficiency_gate() {
    use crate::brain::tools::rsi_propose::RsiProposeTool;

    let tool = RsiProposeTool;
    let result = tool
        .execute(
            serde_json::json!({
                "kind": "tool",
                "name": "ssh_exec",
                "description": "Run SSH commands",
                "rationale": "SSH commands appear frequently across sessions",
                "executor_type": "shell",
                "command": "ssh {{host}} '{{cmd}}'"
            }),
            &ctx(),
        )
        .await
        .unwrap();

    assert!(!result.success, "should reject without efficiency gate");
    let msg = result.error.unwrap_or_default();
    assert!(
        msg.contains("efficiency gate"),
        "expected efficiency gate error, got: {msg}"
    );
}

#[tokio::test]
async fn rsi_propose_tool_accepts_with_efficiency_gate_keywords() {
    use crate::brain::tools::rsi_propose::RsiProposeTool;

    let tool = RsiProposeTool;

    // Test with "token savings" keyword
    let result = tool
        .execute(
            serde_json::json!({
                "kind": "tool",
                "name": "docker_logs",
                "description": "Get Docker container logs",
                "rationale": "TOKEN SAVINGS: eliminates boilerplate of repeated docker logs calls",
                "executor_type": "shell",
                "command": "docker logs {{container}}"
            }),
            &ctx(),
        )
        .await
        .unwrap();

    assert!(
        result.success,
        "should accept with efficiency gate: {:?}",
        result.error
    );

    // Test with "error reduction" keyword
    let result2 = tool
        .execute(
            serde_json::json!({
                "kind": "tool",
                "name": "gh_issue_create",
                "description": "Create GitHub issue",
                "rationale": "ERROR REDUCTION: prevents quoting bugs in issue body",
                "executor_type": "shell",
                "command": "gh issue create --title '{{title}}' --body '{{body}}'"
            }),
            &ctx(),
        )
        .await
        .unwrap();

    assert!(
        result2.success,
        "should accept with error reduction: {:?}",
        result2.error
    );

    // Test with "capability addition" keyword
    let result3 = tool
        .execute(
            serde_json::json!({
                "kind": "tool",
                "name": "parse_json",
                "description": "Parse JSON response",
                "rationale": "CAPABILITY ADDITION: structured output parsing that bash cannot do",
                "executor_type": "shell",
                "command": "cat {{file}} | jq ."
            }),
            &ctx(),
        )
        .await
        .unwrap();

    assert!(
        result3.success,
        "should accept with capability addition: {:?}",
        result3.error
    );
}

#[tokio::test]
async fn rsi_propose_command_not_subject_to_efficiency_gate() {
    use crate::brain::tools::rsi_propose::RsiProposeTool;

    let tool = RsiProposeTool;

    // Commands should not require efficiency gate (only tools do)
    let result = tool
        .execute(
            serde_json::json!({
                "kind": "command",
                "name": "/standup",
                "description": "Daily standup summary",
                "rationale": "User types this repeatedly",
                "prompt": "Summarize yesterday's commits"
            }),
            &ctx(),
        )
        .await
        .unwrap();

    assert!(
        result.success,
        "commands should not require efficiency gate: {:?}",
        result.error
    );
}
