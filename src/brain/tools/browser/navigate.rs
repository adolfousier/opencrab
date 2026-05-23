//! browser_navigate — Navigate to a URL and return page info.

use super::manager::BrowserManager;
use crate::brain::tools::error::Result;
use crate::brain::tools::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

pub struct BrowserNavigateTool {
    manager: Arc<BrowserManager>,
}

impl BrowserNavigateTool {
    pub fn new(manager: Arc<BrowserManager>) -> Self {
        Self { manager }
    }

    /// Best-effort URL equivalence for the same-URL re-navigation guard.
    /// Trailing slash + fragment are commonly different between what the
    /// agent passes and what the browser settled on; trim both before
    /// comparing. Keep query strings — they often change page state.
    fn urls_equivalent(a: &str, b: &str) -> bool {
        let strip = |u: &str| -> String {
            let mut s = u.split('#').next().unwrap_or(u).to_string();
            if s.ends_with('/') && s.matches('/').count() > 3 {
                s.pop();
            }
            s
        };
        strip(a) == strip(b)
    }
}

#[async_trait]
impl Tool for BrowserNavigateTool {
    fn name(&self) -> &str {
        "browser_navigate"
    }

    fn description(&self) -> &str {
        "Navigate to a URL in the browser. Returns the page title, final URL \
         (after redirects), and an automatic screenshot of the page. \
         Supports both headless and headed (visible) mode."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "URL to navigate to"
                },
                "headless": {
                    "type": "boolean",
                    "description": "Run in headless mode (no visible window). Defaults to true. Set to false to see the browser."
                }
            },
            "required": ["url"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::Network]
    }

    fn requires_approval(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, context: &ToolExecutionContext) -> Result<ToolResult> {
        let url = match input["url"].as_str() {
            Some(u) if !u.is_empty() => u,
            _ => return Ok(ToolResult::error("'url' is required".into())),
        };

        // Switch headless/headed mode if requested
        if let Some(headless) = input["headless"].as_bool() {
            self.manager.set_headless(headless).await;
        }

        let page = match self
            .manager
            .get_or_create_session_page(context.session_id)
            .await
        {
            Ok(p) => p,
            Err(e) => return Ok(ToolResult::error(format!("Browser error: {e}"))),
        };

        // Short-circuit when the agent re-navigates to the same URL it's
        // already on. The 2026-05-23 09:04 logs show the agent calling
        // browser_navigate to the SAME URL on iterations 1, 16, and 25
        // — each one waiting 3s for network-idle and returning identical
        // output. Treat it as the no-op it is and tell the agent to take
        // a different action.
        if let Ok(Some(current)) = page.url().await
            && Self::urls_equivalent(&current, url)
        {
            return Ok(ToolResult::error(format!(
                "Already on '{url}' — re-navigating to the same URL won't change \
                 anything. If the previous page state is stale, use `browser_eval` \
                 with `\"window.location.reload()\"`. Otherwise take a different \
                 action: click, type, or navigate to a different URL."
            )));
        }

        if let Err(e) = page.goto(url).await {
            return Ok(ToolResult::error(format!("Navigation failed: {e}")));
        }

        // Wait for the navigation to settle. `wait_for_navigation()` only
        // resolves on the CDP `load` event, which fires before paint and
        // before any JS hydration completes — screenshots and clicks issued
        // immediately after would land on a blank/half-rendered page (the
        // observed "stuck on initial window" symptom). The
        // `wait_for_network_almost_idle_with_timeout` helper from chromey
        // (already proven in click.rs:87) waits until network requests
        // settle, which correlates much more reliably with "page is
        // interactable". 3s timeout matches the existing click flow.
        if let Err(e) = page
            .wait_for_network_almost_idle_with_timeout(std::time::Duration::from_secs(3))
            .await
        {
            tracing::debug!(
                "browser_navigate: network-idle wait timed out after goto({url}) (proceeding anyway): {e}"
            );
        }

        let title = match page.get_title().await {
            Ok(t) => t.unwrap_or_default(),
            Err(e) => {
                tracing::warn!("browser_navigate: get_title failed after goto({url}): {e}");
                String::new()
            }
        };
        let final_url = match page.url().await {
            Ok(u) => u.unwrap_or_default(),
            Err(e) => {
                tracing::warn!("browser_navigate: page.url() failed after goto({url}): {e}");
                String::new()
            }
        };

        let mut result = ToolResult::success(format!("Navigated to: {final_url}\nTitle: {title}"));

        // Auto-screenshot: give the model vision of the page after navigation
        self.manager
            .attach_screenshot(context.session_id, &mut result)
            .await;

        Ok(result)
    }
}
