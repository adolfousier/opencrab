//! Regression (#936): sub-agent IDs survive context compaction.
//!
//! Compaction replaces the conversation with a summary. The summary carried the
//! `spawn_agent` call but not the returned agent id in any form the model could
//! use, so after compaction it could no longer `wait_agent`, `send_input`,
//! `resume_agent` or `close_agent`. The children ran to completion and their
//! results were never collected.
//!
//! The fix appends a table of still-alive agents to the summary. These tests
//! pin what belongs in it — and, just as importantly, what must not.

use crate::brain::tools::subagent::manager::{SubAgent, SubAgentManager, SubAgentState};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

fn agent(id: &str, label: &str, state: SubAgentState) -> SubAgent {
    SubAgent {
        state,
        input_tx: None,
        ..SubAgent::new(
            id.to_string(),
            label.to_string(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        )
    }
}

#[test]
fn no_agents_means_no_block_at_all() {
    // Every compaction on every session would otherwise carry a dead heading.
    let mgr = SubAgentManager::new();
    assert!(mgr.format_running_for_compaction().is_none());
}

#[test]
fn a_running_agent_is_listed_with_the_id_the_tools_need() {
    let mgr = SubAgentManager::new();
    mgr.insert(agent("abc12345", "research-foo", SubAgentState::Running));

    let block = mgr
        .format_running_for_compaction()
        .expect("a running agent must produce a block");
    assert!(
        block.contains("abc12345"),
        "the id is the whole point — without it the tools cannot be called:\n{block}"
    );
    assert!(block.contains("research-foo"), "{block}");
    assert!(block.contains("Running"), "{block}");
}

#[test]
fn an_agent_awaiting_input_is_listed_too() {
    // This is the state that most needs surfacing: it is blocked ON the parent,
    // so losing its id strands it permanently.
    let mgr = SubAgentManager::new();
    mgr.insert(agent(
        "def45678",
        "needs-answer",
        SubAgentState::AwaitingInput,
    ));

    let block = mgr.format_running_for_compaction().expect("must be listed");
    assert!(block.contains("def45678"), "{block}");
    assert!(block.contains("AwaitingInput"), "{block}");
}

#[test]
fn finished_agents_are_left_out() {
    // Listing a dead agent invites the model to wait on something that will
    // never report, which is its own hang.
    let mgr = SubAgentManager::new();
    mgr.insert(agent("done0001", "completed-one", SubAgentState::Completed));
    mgr.insert(agent("done0002", "cancelled-one", SubAgentState::Cancelled));
    mgr.insert(agent(
        "done0003",
        "failed-one",
        SubAgentState::Failed("boom".into()),
    ));

    assert!(
        mgr.format_running_for_compaction().is_none(),
        "only non-terminal agents belong in the preamble"
    );
}

#[test]
fn a_mixed_roster_lists_only_the_live_ones() {
    let mgr = SubAgentManager::new();
    mgr.insert(agent("live0001", "still-going", SubAgentState::Running));
    mgr.insert(agent("dead0001", "already-done", SubAgentState::Completed));

    let block = mgr.format_running_for_compaction().expect("one is alive");
    assert!(block.contains("live0001"), "{block}");
    assert!(
        !block.contains("dead0001"),
        "a finished agent must not appear:\n{block}"
    );
}

#[test]
fn every_live_agent_gets_a_row() {
    // A turn can spawn several; losing any one of them is the reported bug.
    let mgr = SubAgentManager::new();
    for n in 0..3 {
        mgr.insert(agent(
            &format!("id00000{n}"),
            &format!("worker-{n}"),
            SubAgentState::Running,
        ));
    }

    let block = mgr.format_running_for_compaction().expect("three alive");
    for n in 0..3 {
        assert!(
            block.contains(&format!("id00000{n}")),
            "missing {n}:\n{block}"
        );
    }
    let rows = block.lines().filter(|l| l.contains("| `id")).count();
    assert_eq!(rows, 3, "one row per live agent:\n{block}");
}

#[test]
fn the_block_names_the_tools_the_ids_are_for() {
    // The table alone does not tell the model what to do with it, and the whole
    // failure was the model not resuming its children.
    let mgr = SubAgentManager::new();
    mgr.insert(agent("abc12345", "research-foo", SubAgentState::Running));

    let block = mgr.format_running_for_compaction().expect("block");
    for tool in ["wait_agent", "send_input", "resume_agent", "close_agent"] {
        assert!(block.contains(tool), "must mention {tool}:\n{block}");
    }
    assert!(
        block.to_lowercase().contains("duplicate"),
        "must warn against re-spawning what is already running:\n{block}"
    );
}
