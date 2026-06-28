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

/// Enrich the metadata snippet for tool failures so RSI / SQL
/// analyses can categorize them by subsystem. Without this every
/// `bash` failure lands in the same dimension and the question
/// "which kind of bash fails most?" has no answer (issue #132).
///
/// Today this only enriches `bash`: we append `| cmd=<text>` so
/// queries like `WHERE meta LIKE '%cmd=git%'` work. The command
/// is truncated independently of the error snippet so a giant
/// command line can't crowd out the error itself; the full meta
/// then gets the 500-char outer cap below.
pub(crate) fn enrich_metadata(
    tool_name: &str,
    error_snippet: Option<&str>,
    tool_input: Option<&serde_json::Value>,
) -> Option<String> {
    let snippet = error_snippet.unwrap_or("");
    let cmd_suffix = if tool_name == "bash"
        && let Some(input) = tool_input
        && let Some(cmd) = input.get("command").and_then(|v| v.as_str())
        && !cmd.is_empty()
    {
        // Cap the command alone at 300 chars so the error snippet
        // still gets meaningful room within the outer 500-char cap.
        //
        // Enriched on BOTH success and failure so RSI's
        // success-pattern detection pass can group bash invocations
        // by subsystem (gh, git, docker, ...) — without this, only
        // failures would carry the command and RSI couldn't see
        // patterns in the (much more common) successful calls.
        let cmd_short: String = cmd.chars().take(300).collect();
        Some(format!(" | cmd={cmd_short}"))
    } else {
        None
    };
    match (snippet.is_empty(), cmd_suffix) {
        (true, None) => None,
        // Strip the " | " prefix the suffix carries (used when
        // joining with a snippet). With no snippet to join against
        // we emit a bare `cmd=...` so downstream LIKE queries can
        // still match without the leading separator.
        (true, Some(suffix)) => Some(suffix.strip_prefix(" | ").unwrap_or(&suffix).to_string()),
        (false, None) => Some(snippet.to_string()),
        (false, Some(suffix)) => Some(format!("{snippet}{suffix}")),
    }
}

impl AgentService {
    /// Fire-and-forget recording of a tool execution to the feedback ledger.
    /// Never blocks, never fails visibly — if the DB is unavailable or the
    /// write fails we just log and move on.
    ///
    /// `tool_input` is the JSON the agent passed to the tool. We use it
    /// to enrich failure metadata for subsystem-specific analysis (e.g.
    /// appending `cmd=...` for bash failures so RSI can group by
    /// `git` / `python` / `docker` prefixes). Pass `None` when the
    /// call site doesn't have a meaningful input (e.g. user-denial
    /// before execution).
    pub(super) fn record_tool_feedback(
        &self,
        session_id: Uuid,
        tool_name: &str,
        tool_input: Option<&serde_json::Value>,
        success: bool,
        error_snippet: Option<&str>,
    ) {
        let pool = self.context.pool();
        let sid = session_id.to_string();
        let tname = tool_name.to_string();
        let enriched = enrich_metadata(tool_name, error_snippet, tool_input);
        let meta = enriched.map(|s| s.chars().take(500).collect::<String>());
        // Recoverable / environmental failures (stale-hash retry, channel not
        // connected, user-cancelled prompt) are NOT tool defects. Record them
        // under a non-`tool_` event so they stay queryable but OUT of the
        // success-rate denominator — otherwise they drove the RSI to ban
        // working built-in tools (#236).
        let recoverable = !success
            && crate::brain::feedback_policy::is_recoverable_tool_failure(tool_name, error_snippet);
        tokio::spawn(async move {
            let repo = crate::db::repository::FeedbackLedgerRepository::new(pool);
            let event = if success {
                "tool_success"
            } else if recoverable {
                "recoverable_failure"
            } else {
                "tool_failure"
            };
            let val = if success { 1.0 } else { 0.0 };
            if let Err(e) = repo.record(&sid, event, &tname, val, meta.as_deref()).await {
                tracing::debug!("feedback ledger write failed: {e}");
            }
        });
    }

    /// Fire-and-forget recording of a pre-execution tool miss (#214): the model
    /// called a tool that didn't resolve, or with arguments that failed
    /// validation, so the tool never ran. Recorded as `discovery_miss` (NOT a
    /// `tool_`-prefixed event) so it stays OUT of the success-rate denominator
    /// in `stats_by_dimension("tool_")`, while staying visible in the
    /// event-type breakdown and queryable for "what is the model guessing
    /// blind" analysis. `dimension` is the tool name.
    pub(super) fn record_tool_discovery_miss(
        &self,
        session_id: Uuid,
        tool_name: &str,
        tool_input: Option<&serde_json::Value>,
        error_snippet: Option<&str>,
    ) {
        let pool = self.context.pool();
        let sid = session_id.to_string();
        let tname = tool_name.to_string();
        let enriched = enrich_metadata(tool_name, error_snippet, tool_input);
        let meta = enriched.map(|s| s.chars().take(500).collect::<String>());
        tokio::spawn(async move {
            let repo = crate::db::repository::FeedbackLedgerRepository::new(pool);
            if let Err(e) = repo
                .record(&sid, "discovery_miss", &tname, 0.0, meta.as_deref())
                .await
            {
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
