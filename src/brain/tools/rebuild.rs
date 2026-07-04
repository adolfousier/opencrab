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

        match crate::cron::schedule_background_rebuild(pool, context.session_id, None).await {
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
