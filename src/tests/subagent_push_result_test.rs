//! A finished sub-agent reports to the session that spawned it (#1036).
//!
//! The push exists for the fire-and-forget case the tool description
//! advertises: the parent turn ends, the child finishes later, and without a
//! push its output would sit in the manager map with nobody to read it. A
//! parent already blocked on `wait_agent` receives the output as that tool's
//! result, so pushing as well would deliver it twice.

use crate::brain::tools::subagent::manager::{SubAgent, SubAgentManager, SubAgentState};
use crate::brain::tools::subagent::spawn::completion_message;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

fn manager_with_agent(id: &str) -> SubAgentManager {
    let manager = SubAgentManager::new();
    manager.insert(SubAgent {
input_tx: None,
..SubAgent::new(
        id.to_string(),
        "worker".to_string(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    )
    });
    manager
}

#[test]
fn an_unwatched_agent_pushes_its_result() {
    let manager = manager_with_agent("a1");
    assert!(
        manager.mark_completed("a1", "done".to_string()),
        "nobody is waiting, so nothing else would ever carry the output"
    );
}

#[test]
fn an_agent_someone_waits_on_does_not_push() {
    let manager = manager_with_agent("a2");
    assert!(manager.enter_wait("a2"));

    assert!(
        !manager.mark_completed("a2", "done".to_string()),
        "wait_agent returns this output as its own result"
    );
}

#[test]
fn the_push_resumes_once_the_waiter_leaves() {
    // A wait that times out releases its registration, so a later completion
    // is unwatched again and must push.
    let manager = manager_with_agent("a3");
    manager.enter_wait("a3");
    manager.leave_wait("a3");

    assert!(manager.mark_completed("a3", "done".to_string()));
}

#[test]
fn concurrent_waiters_each_hold_the_push_back() {
    let manager = manager_with_agent("a4");
    manager.enter_wait("a4");
    manager.enter_wait("a4");
    manager.leave_wait("a4");

    assert!(
        !manager.mark_completed("a4", "done".to_string()),
        "one waiter remains"
    );
}

#[test]
fn a_failure_nobody_waits_on_still_pushes() {
    // A failure is a result too: the parent has to know not to wait on work
    // that will never arrive.
    let manager = manager_with_agent("a5");
    assert!(manager.mark_failed("a5", "boom".to_string()));
}

#[test]
fn a_failure_someone_waits_on_does_not_push() {
    let manager = manager_with_agent("a6");
    manager.enter_wait("a6");

    assert!(!manager.mark_failed("a6", "boom".to_string()));
}

#[test]
fn waiting_on_an_unknown_agent_reports_no_registration() {
    // The guard must not leave a decrement outstanding for an agent that was
    // never there, which would corrupt another agent's count.
    let manager = SubAgentManager::new();
    assert!(!manager.enter_wait("ghost"));
}

#[test]
fn leaving_a_wait_never_underflows() {
    let manager = manager_with_agent("a7");
    manager.leave_wait("a7");
    manager.leave_wait("a7");

    assert!(
        manager.mark_completed("a7", "done".to_string()),
        "count floors at zero rather than wrapping"
    );
}

#[test]
fn a_completion_message_carries_the_output_and_forbids_a_respawn() {
    let msg = completion_message("build docs", "abc123", Ok("42 pages written"));

    assert!(msg.context_text.contains("42 pages written"));
    assert!(msg.context_text.contains("completed"));
    assert!(
        msg.context_text.contains("Do not re-spawn"),
        "the message IS the result; re-running would duplicate the work"
    );
    assert!(msg.display_text.contains("build docs"));
}

#[test]
fn a_failure_message_says_so_rather_than_implying_success() {
    let msg = completion_message("deploy", "def456", Err("connection refused"));

    assert!(msg.context_text.contains("connection refused"));
    assert!(msg.context_text.contains("failed"));
    assert!(
        msg.context_text
            .contains("Do not assume the work was completed"),
        "an absent result must not read as success"
    );
    assert!(msg.display_text.contains("deploy"));
}

#[test]
fn a_long_output_keeps_its_tail() {
    // The conclusion matters more than the opening, matching how the detached
    // command path truncates.
    let long: String = std::iter::repeat_n('x', 5000)
        .chain("THE-CONCLUSION".chars())
        .collect();
    let msg = completion_message("big", "ghi789", Ok(&long));

    assert!(msg.context_text.contains("THE-CONCLUSION"));
    assert!(msg.context_text.contains("truncated"));
    assert!(msg.context_text.len() < long.len());
}
