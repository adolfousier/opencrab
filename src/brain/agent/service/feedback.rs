//! Fire-and-forget feedback-ledger writes.
//!
//! Extracted from `tool_loop.rs` (was lines 137-181) as part of the
//! 2026-05-04 Linor-flagged refactor: `tool_loop.rs` was 4,047 lines.
//! Both methods spawn a detached task that writes to the
//! `FeedbackLedgerRepository`; neither blocks the caller and write
//! failures only log at `debug!`. This is the RSI feedback signal
//! source — the data the hourly RSI loop later analyses for tool /
//! provider success patterns.
//!
//! Behaviour is unchanged from the pre-extraction version. Lives in
//! the same `impl AgentService { ... }` block as the rest of the
//! service so call sites inside `tool_loop.rs` keep using
//! `self.record_tool_feedback(...)` / `self.record_provider_feedback(...)`
//! exactly as before.

use super::builder::AgentService;
use uuid::Uuid;

impl AgentService {
    /// Fire-and-forget recording of a tool execution to the feedback ledger.
    /// Never blocks, never fails visibly — if the DB is unavailable or the
    /// write fails we just log and move on.
    pub(super) fn record_tool_feedback(
        &self,
        session_id: Uuid,
        tool_name: &str,
        success: bool,
        error_snippet: Option<&str>,
    ) {
        let pool = self.context.pool();
        let sid = session_id.to_string();
        let tname = tool_name.to_string();
        let meta = error_snippet.map(|s| s.chars().take(500).collect::<String>());
        tokio::spawn(async move {
            let repo = crate::db::repository::FeedbackLedgerRepository::new(pool);
            let event = if success {
                "tool_success"
            } else {
                "tool_failure"
            };
            let val = if success { 1.0 } else { 0.0 };
            if let Err(e) = repo.record(&sid, event, &tname, val, meta.as_deref()).await {
                tracing::debug!("feedback ledger write failed: {e}");
            }
        });
    }

    /// Fire-and-forget recording of a provider error to the feedback ledger.
    pub(super) fn record_provider_feedback(
        &self,
        session_id: Uuid,
        event_type: &str,
        dimension: &str,
        metadata: Option<&str>,
    ) {
        let pool = self.context.pool();
        let sid = session_id.to_string();
        let et = event_type.to_string();
        let dim = dimension.to_string();
        let meta = metadata.map(|s| s.chars().take(500).collect::<String>());
        tokio::spawn(async move {
            let repo = crate::db::repository::FeedbackLedgerRepository::new(pool);
            if let Err(e) = repo.record(&sid, &et, &dim, 0.0, meta.as_deref()).await {
                tracing::debug!("feedback ledger write failed: {e}");
            }
        });
    }
}
