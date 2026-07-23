//! Regression test for the perpetual-resume loop (#729).
//!
//! A genuine user turn is tracked in `pending_requests` while it runs so a
//! crash/restart mid-turn can recover it. A *resume* turn — the recovery
//! itself — must NOT be tracked: if it were, an interrupted resume (cancelled
//! by a new message, killed on another restart, or a crash before the cleanup
//! delete) would leave its own row behind and resume the same already-done
//! session on every subsequent startup, with rows piling up.
//!
//! The delete on the normal path runs even on graceful cancellation, so the
//! difference is only observable *mid-turn*: was a row ever inserted at all?
//! This test probes the `pending_requests` table from inside a tool call and
//! asserts a row is present during a normal turn and absent during a resume.

use crate::brain::agent::service::AgentService;
use crate::brain::tools::{Tool, ToolExecutionContext, ToolRegistry, ToolResult};
use crate::db::{Database, PendingRequestRepository, Pool};
use crate::services::{ServiceContext, SessionService};
use crate::tests::agent_service_mocks::MockProviderWithNamedTool;
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Tool that, when executed, records whether the running session has a live
/// `pending_requests` row. Fires once per turn from the mock provider.
struct PendingProbeTool {
    pool: Pool,
    observed_row: Arc<AtomicBool>,
    ran: Arc<AtomicUsize>,
}

#[async_trait]
impl Tool for PendingProbeTool {
    fn name(&self) -> &str {
        "pending_probe"
    }

    fn description(&self) -> &str {
        "Probes the pending_requests table for the running session"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({ "type": "object", "properties": {} })
    }

    fn capabilities(&self) -> Vec<crate::brain::tools::ToolCapability> {
        vec![]
    }

    fn requires_approval(&self) -> bool {
        false
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        context: &ToolExecutionContext,
    ) -> crate::brain::tools::Result<ToolResult> {
        let repo = PendingRequestRepository::new(self.pool.clone());
        let has_row = repo
            .find_latest_for_session(context.session_id)
            .await
            .ok()
            .flatten()
            .is_some();
        self.observed_row.store(has_row, Ordering::SeqCst);
        self.ran.fetch_add(1, Ordering::SeqCst);
        Ok(ToolResult::success("probed".to_string()))
    }
}

async fn probe_service(
    pool: Pool,
    context: ServiceContext,
) -> (Arc<AgentService>, Arc<AtomicBool>, Arc<AtomicUsize>) {
    let observed = Arc::new(AtomicBool::new(false));
    let ran = Arc::new(AtomicUsize::new(0));
    let registry = ToolRegistry::new();
    registry.register(Arc::new(PendingProbeTool {
        pool,
        observed_row: observed.clone(),
        ran: ran.clone(),
    }));
    let svc = Arc::new(
        AgentService::new_for_test(
            Arc::new(MockProviderWithNamedTool::new("pending_probe")),
            context,
        )
        .await
        .with_tool_registry(Arc::new(registry))
        .with_auto_approve_tools(true),
    );
    (svc, observed, ran)
}

#[tokio::test]
async fn normal_turn_is_tracked_resume_turn_is_not() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let pool = db.pool().clone();
    let context = ServiceContext::new(pool.clone());

    let session_service = SessionService::new(context.clone());
    let session = session_service
        .create_session(Some("Resume regression".to_string()))
        .await
        .unwrap();

    // A normal user turn: a pending row must exist while the tool runs.
    let (svc_normal, normal_observed, normal_ran) =
        probe_service(pool.clone(), context.clone()).await;
    svc_normal
        .send_message_with_tools_and_mode(session.id, "do the thing".to_string(), None, None)
        .await
        .unwrap();
    assert_eq!(
        normal_ran.load(Ordering::SeqCst),
        1,
        "probe should run once"
    );
    assert!(
        normal_observed.load(Ordering::SeqCst),
        "a normal turn must be tracked in pending_requests while it runs"
    );

    // A resume turn: the recovery must NOT insert its own pending row, or an
    // interrupted resume would relaunch this session on every restart (#729).
    let (svc_resume, resume_observed, resume_ran) =
        probe_service(pool.clone(), context.clone()).await;
    svc_resume
        .resume_interrupted_turn(
            session.id,
            "[System: resume]".to_string(),
            None,
            None,
            None,
            None,
            None,
            "tui",
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        resume_ran.load(Ordering::SeqCst),
        1,
        "probe should run once"
    );
    assert!(
        !resume_observed.load(Ordering::SeqCst),
        "a resume turn must NOT be tracked — otherwise it resumes forever (#729)"
    );

    // The table is clean at rest — no debris that would trigger a fresh resume.
    let repo = PendingRequestRepository::new(pool);
    assert!(
        repo.get_interrupted().await.unwrap().is_empty(),
        "no pending rows should survive a completed normal + resume turn"
    );
}
