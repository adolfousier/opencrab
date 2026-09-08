//! Rebuild Tool
//!
//! Lets the agent build OpenCrabs from source and reload automatically.
//! The build runs in the BACKGROUND via a one-shot cron job
//! (`schedule_background_rebuild`) so the agent turn isn't blocked for the
//! minutes a release build takes; the scheduler exec-restarts into the new
//! binary when the build finishes.

use super::error::Result;
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;

/// Map an originating channel + chat id (+ optional forum topic) to a cron
/// `deliver_to` target so the background rebuild can report completion and
/// failure into the chat that asked (#305). Only channels with a scheduler
/// delivery arm map; the TUI has its own notifier (#304) and everything else
/// returns None.
///
/// Telegram forum topics ride the #1451 grammar (`telegram:chat:thread`,
/// parsed by the scheduler's `parse_telegram_target`): when the pending
/// request carries an origin topic the report lands IN that topic (#1457),
/// otherwise the plain `telegram:chat` form keeps the historical default
/// topic delivery. Discord/Slack have no thread component here.
pub(crate) fn rebuild_deliver_target(
    channel: &str,
    chat_id: Option<&str>,
    thread_id: Option<&str>,
) -> Option<String> {
    let chat_id = chat_id?.trim();
    if chat_id.is_empty() {
        return None;
    }
    match channel {
        "telegram" => {
            let thread = thread_id.map(str::trim).filter(|t| !t.is_empty());
            match thread {
                Some(t) => Some(format!("telegram:{chat_id}:{t}")),
                None => Some(format!("telegram:{chat_id}")),
            }
        }
        "discord" | "slack" => Some(format!("{channel}:{chat_id}")),
        _ => None,
    }
}

/// Agent-callable tool that schedules a background rebuild from source.
pub struct RebuildTool;

impl RebuildTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RebuildTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for RebuildTool {
    fn name(&self) -> &str {
        "rebuild"
    }

    fn description(&self) -> &str {
        "Build OpenCrabs from source (cargo build --release) in the BACKGROUND and auto-reload \
         when it's done. MAINTAINER path, rare: only for applying LOCAL SOURCE EDITS. To \
         upgrade to the latest published release use the `evolve` tool instead (rebuild \
         compiles what is on disk; evolve fetches what was released). Returns \
         immediately — the build runs out-of-band (it does not block you), and OpenCrabs \
         exec-restarts into the new binary automatically when the build finishes, resuming \
         this session. On build failure a message is delivered; nothing restarts."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::SystemModification]
    }

    async fn execute(&self, _input: Value, context: &ToolExecutionContext) -> Result<ToolResult> {
        // The build runs in the BACKGROUND via a one-shot cron job, so this
        // turn (and the user's session) isn't blocked for the minutes a
        // release build takes. The scheduler builds from source and
        // exec-restarts into the new binary when ready, resuming this session.
        let pool = match context.service_context.as_ref() {
            Some(ctx) => ctx.pool(),
            None => {
                return Ok(ToolResult::error(
                    "rebuild: no service context available to schedule the background build"
                        .to_string(),
                ));
            }
        };

        // Resolve WHERE this turn came from so the build's completion and
        // failure notices land back in that chat (#305). The pending-request
        // row for the current turn is alive while this tool runs and carries
        // channel + chat id. TUI turns map to None: the TUI has its own
        // failure notifier and the post-restart wake-up covers success.
        let deliver_to = match crate::db::PendingRequestRepository::new(pool.clone())
            .find_latest_for_session(context.session_id)
            .await
        {
            Ok(Some(req)) => {
                let target = rebuild_deliver_target(
                    &req.channel,
                    req.channel_chat_id.as_deref(),
                    req.channel_thread_id.as_deref(),
                );
                match &target {
                    Some(t) => tracing::info!("rebuild: status will be delivered to {t}"),
                    None => tracing::debug!(
                        "rebuild: no channel delivery target for channel '{}'",
                        req.channel
                    ),
                }
                target
            }
            Ok(None) => None,
            Err(e) => {
                tracing::warn!("rebuild: pending-request lookup failed (no delivery): {e}");
                None
            }
        };

        match crate::cron::schedule_background_rebuild(pool, context.session_id, deliver_to).await {
            Ok(()) => Ok(ToolResult::success(
                "🔨 Rebuild scheduled in the background. The build runs out-of-band; \
                 OpenCrabs will reload into the new binary automatically when it's \
                 done — no need to wait, you can keep working."
                    .to_string(),
            )),
            Err(e) => Ok(ToolResult::error(format!(
                "Failed to schedule background rebuild: {e}"
            ))),
        }
    }
}
