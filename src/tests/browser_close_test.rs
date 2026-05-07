//! Tests for `browser_close` — the tool that closes the current
//! session's CDP page so it doesn't persist forever.
//!
//! Pre-fix, pages stored in the manager's HashMap were never cleaned
//! up unless the entire process exited. The agent had no way to ask
//! "I'm done with this tab, close it" — so the Chrome window stayed
//! visible on the user's desktop and a fresh agent invocation in the
//! same session reused the stale page from the prior task.
//!
//! These tests don't launch Chrome — they exercise the tool surface
//! (name / description / schema / dispatch on missing session) and
//! the underlying `BrowserManager::close_page` HashMap removal.

#![cfg(feature = "browser")]

use crate::brain::tools::browser::{BrowserCloseTool, BrowserManager};
use crate::brain::tools::{Tool, ToolExecutionContext};
use std::sync::Arc;
use uuid::Uuid;

#[test]
fn tool_name_is_browser_close() {
    let mgr = Arc::new(BrowserManager::new());
    let tool = BrowserCloseTool::new(mgr);
    assert_eq!(tool.name(), "browser_close");
}

#[test]
fn tool_does_not_require_approval() {
    // Closing a tab is non-destructive of user data; should not
    // gate on the approval system.
    let mgr = Arc::new(BrowserManager::new());
    let tool = BrowserCloseTool::new(mgr);
    assert!(!tool.requires_approval());
}

#[test]
fn tool_input_schema_is_object_with_no_required_fields() {
    let mgr = Arc::new(BrowserManager::new());
    let tool = BrowserCloseTool::new(mgr);
    let schema = tool.input_schema();
    assert_eq!(
        schema["type"], "object",
        "schema must be an object so the model can call with `{{}}`"
    );
    assert!(
        schema.get("required").is_none()
            || schema["required"].as_array().is_none_or(|a| a.is_empty()),
        "browser_close takes no required fields — session_id is implicit in context",
    );
}

#[tokio::test]
async fn dispatch_with_no_session_returns_idempotent_success() {
    // Pre-condition: fresh manager, no pages opened. The agent may
    // call browser_close defensively (e.g. start of a workflow) so
    // calling it on an empty session must NOT return an error —
    // otherwise the agent thinks something failed and retries.
    let mgr = Arc::new(BrowserManager::new());
    let tool = BrowserCloseTool::new(mgr);
    let ctx = ToolExecutionContext::new(Uuid::new_v4());

    let res = tool.execute(serde_json::json!({}), &ctx).await.unwrap();
    assert!(
        res.success,
        "browser_close on no-page session must be idempotent success, got error: {}",
        res.output
    );
    assert!(
        res.output.contains("nothing to close") || res.output.contains("No browser tab"),
        "output must explain there was nothing to close so the agent doesn't think it succeeded \
         in closing a real tab: {}",
        res.output,
    );
}

#[tokio::test]
async fn close_page_for_unknown_name_returns_false() {
    // Lower-level invariant — `close_page` returns false when the
    // name isn't in the HashMap so callers can distinguish
    // "actually closed something" from "no-op".
    let mgr = BrowserManager::new();
    let closed = mgr.close_page("session-does-not-exist").await;
    assert!(
        !closed,
        "close_page must return false for unknown session names"
    );
}

#[tokio::test]
async fn close_page_for_unknown_session_uuid_returns_false() {
    // Same invariant via the session-keyed wrapper.
    let mgr = BrowserManager::new();
    let closed = mgr.close_page_for_session(Uuid::new_v4()).await;
    assert!(
        !closed,
        "close_page_for_session must return false when no page is open"
    );
}
