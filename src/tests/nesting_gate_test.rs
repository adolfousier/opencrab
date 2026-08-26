//! #1195 task 5 - worker purity via `allow_nested`.
//!
//! Pins the three manager-lookup semantics (restricted child denied,
//! unrestricted child allowed, unregistered session allowed) and the
//! plan-worker spawn JSON carrying `allow_nested: false` by default.

use crate::brain::tools::subagent::SubAgent;
use crate::brain::tools::subagent::SubAgentManager;
use tokio::sync::mpsc;
use uuid::Uuid;

fn make_agent(session_id: Uuid, allow_nested: bool) -> SubAgent {
    let (tx, _rx) = mpsc::unbounded_channel::<String>();
    SubAgent {
        allow_nested,
        input_tx: Some(tx),
        ..SubAgent::new(
            format!("agent-{}", Uuid::new_v4().simple()),
            "test".to_string(),
            session_id,
            Uuid::new_v4(),
        )
    }
}

#[test]
fn restricted_child_cannot_nest() {
    let mgr = SubAgentManager::default();
    let sid = Uuid::new_v4();
    mgr.insert(make_agent(sid, false));
    assert!(
        !mgr.nesting_allowed_for_session(sid),
        "a child spawned with allow_nested=false must be denied nesting"
    );
}

#[test]
fn unrestricted_child_may_nest() {
    let mgr = SubAgentManager::default();
    let sid = Uuid::new_v4();
    mgr.insert(make_agent(sid, true));
    assert!(
        mgr.nesting_allowed_for_session(sid),
        "allow_nested=true children keep nesting rights"
    );
}

#[test]
fn unknown_session_is_root_and_allowed() {
    let mgr = SubAgentManager::default();
    // No agents registered at all.
    assert!(mgr.nesting_allowed_for_session(Uuid::new_v4()));
    // Registered siblings do not affect an unrelated session id.
    let other = Uuid::new_v4();
    mgr.insert(make_agent(other, false));
    assert!(
        mgr.nesting_allowed_for_session(Uuid::new_v4()),
        "sessions without a manager entry are roots - always unrestricted"
    );
    assert!(!mgr.nesting_allowed_for_session(other));
}

#[test]
fn plan_worker_spawn_json_is_pure() {
    // The plan-worker spawn path must request a non-nesting child unless the
    // operator opted out via [agent] plan_worker_allow_nested (#1195). Pin
    // both halves of that wiring by their unique source strings.
    let src = include_str!("../brain/tools/plan_tool.rs");
    assert!(
        src.contains("\"allow_nested\": worker_nesting,"),
        "plan workers must pass allow_nested explicitly (worker purity, #1195)"
    );
    assert!(
        src.contains("c.agent.plan_worker_allow_nested"),
        "worker nesting must be gated on [agent] plan_worker_allow_nested"
    );
    // #1229 drift guard: plan workers are READ-ONLY by default (#1173 grant);
    // mutations stay with the parent unless [agent] plan_worker_allow_write.
    assert!(
        src.contains("\"read_only\": worker_read_only,"),
        "plan workers must pass read_only explicitly (drift guard)"
    );
    assert!(
        src.contains("c.agent.plan_worker_allow_write"),
        "worker read-only mode must be gated on [agent] plan_worker_allow_write"
    );
}
