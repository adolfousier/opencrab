//! browser_close — Close the current session's browser page.
//!
//! Without this, pages persisted in the manager's HashMap forever. The
//! CDP session stayed open, the Chrome tab stayed visible on the user's
//! desktop, and a fresh agent invocation reused the stale page from a
//! prior task. The tool exposes `BrowserManager::close_page_for_session`
//! to the agent so it can clean up explicitly when a browser task is
//! complete.

use super::manager::BrowserManager;
use crate::brain::tools::error::Result;
use crate::brain::tools::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

pub struct BrowserCloseTool {
    manager: Arc<BrowserManager>,
}

impl BrowserCloseTool {
    pub fn new(manager: Arc<BrowserManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserCloseTool {
    fn name(&self) -> &str {
        "browser_close"
    }

    fn description(&self) -> &str {
        "Close the current session's browser tab and free its CDP session. \
         Call this when you're done with a browser task — otherwise the tab \
         stays open and a future browser action in the same session will \
         reuse the stale page. Idempotent: safe to call when no tab is open."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![]
    }

    fn requires_approval(&self) -> bool {
        false
    }

    async fn execute(&self, _input: Value, context: &ToolExecutionContext) -> Result<ToolResult> {
        let closed = self
            .manager
            .close_page_for_session(context.session_id)
            .await;
        if closed {
            Ok(ToolResult::success(
                "Browser tab closed for this session.".to_string(),
            ))
        } else {
            // Idempotent: returning success when there was nothing to
            // close avoids the agent thinking it failed and retrying.
            Ok(ToolResult::success(
                "No browser tab was open for this session — nothing to close.".to_string(),
            ))
        }
    }
}
