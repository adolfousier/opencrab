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
