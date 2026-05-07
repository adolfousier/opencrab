//! Browser end-to-end integration tests — ALL `#[ignore]`'d so they
//! never run in CI by default. Launches a real headless Chrome and
//! exercises the full CDP path against `https://example.com`.
//!
//! Run manually:
//!
//!     cargo test --features browser --lib browser_e2e -- --ignored
//!
//! Each test pins a specific bug fix from the 2026-05-07 browser
//! resilience pass (commits 2d09065e, 7f58c6f9, 85f5a73b, and the
//! browser_close addition). They're marked `#[ignore]` because:
//!   * Chrome launches add ~2-5s per test
//!   * They need network access (example.com)
//!   * They depend on a Chrome/Chromium binary being installed
//!   * CI runners would need a separate browser-test job to avoid
//!     flake noise in the main suite.

#![cfg(feature = "browser")]

use crate::brain::tools::browser::{
    BrowserCloseTool, BrowserManager, BrowserNavigateTool, BrowserScreenshotTool,
};
use crate::brain::tools::{Tool, ToolExecutionContext};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

const TEST_URL: &str = "https://example.com";

/// Pins the bug-#1 fix (commit 2d09065e): after `browser_navigate`,
/// the auto-screenshot must be a NON-BLANK image. Pre-fix,
/// `wait_for_navigation()` returned on the CDP `load` event before
/// paint, so the screenshot captured a blank/half-rendered page.
#[tokio::test]
#[ignore = "launches real Chrome — opt-in via `cargo test -- --ignored browser_e2e`"]
async fn navigate_then_screenshot_is_not_blank() {
    let mgr = Arc::new(BrowserManager::new());
    let nav = BrowserNavigateTool::new(mgr.clone());
    let ctx = ToolExecutionContext::new(Uuid::new_v4());

    let res = nav
        .execute(serde_json::json!({ "url": TEST_URL }), &ctx)
        .await
        .expect("navigate tool must not panic");
    assert!(
        res.success,
        "navigate to example.com must succeed: {}",
        res.output
    );

    // Post-navigate: the auto-screenshot rides along in result.images.
    assert!(
        !res.images.is_empty(),
        "navigate must attach an auto-screenshot to the result"
    );
    // images is Vec<(media_type, base64_data)>; index .1 is the
    // base64-encoded PNG. Heuristic: a real screenshot of example.com
    // is at least 4 KB after PNG compression. A blank/single-color
    // capture from the pre-fix race would be well under 1 KB.
    let (_mime, b64) = &res.images[0];
    let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64.as_bytes())
        .expect("auto-screenshot must be valid base64");
    assert!(
        bytes.len() > 4096,
        "auto-screenshot is suspiciously small ({} bytes) — \
         likely captured a blank page (regression of the 2d09065e fix)",
        bytes.len()
    );

    // Cleanup so the Chrome process doesn't leak between tests.
    let close = BrowserCloseTool::new(mgr);
    let _ = close.execute(serde_json::json!({}), &ctx).await;
}

/// Pins the bug-#2 fix (commit 7f58c6f9): two concurrent screenshot
/// calls in the same session must complete within a reasonable
/// budget. Pre-fix, the manager mutex was held across the awaited
/// CDP screenshot call, so a second concurrent call queued behind
/// the first one's full network round-trip — and worse, any task
/// trying to acquire the same mutex during the screenshot deadlocked.
#[tokio::test]
#[ignore = "launches real Chrome — opt-in via `cargo test -- --ignored browser_e2e`"]
async fn concurrent_screenshots_do_not_deadlock() {
    let mgr = Arc::new(BrowserManager::new());
    let nav = BrowserNavigateTool::new(mgr.clone());
    let shot = Arc::new(BrowserScreenshotTool::new(mgr.clone()));
    let ctx = ToolExecutionContext::new(Uuid::new_v4());

    nav.execute(serde_json::json!({ "url": TEST_URL }), &ctx)
        .await
        .expect("seed navigate must succeed");

    // Fire two screenshot calls in parallel. They share the manager
    // and same session — a regression of the lock-held-across-await
    // bug would either deadlock or serialize, exceeding the timeout.
    let shot_a = shot.clone();
    let ctx_a = ctx.clone();
    let shot_b = shot.clone();
    let ctx_b = ctx.clone();

    let combined = tokio::time::timeout(
        Duration::from_secs(15),
        futures::future::join(
            async move { shot_a.execute(serde_json::json!({}), &ctx_a).await },
            async move { shot_b.execute(serde_json::json!({}), &ctx_b).await },
        ),
    )
    .await;

    let (a, b) = combined.expect(
        "concurrent screenshots took >15s — likely deadlocked behind \
         the manager mutex (regression of the 7f58c6f9 fix)",
    );
    assert!(a.unwrap().success);
    assert!(b.unwrap().success);

    let close = BrowserCloseTool::new(mgr);
    let _ = close.execute(serde_json::json!({}), &ctx).await;
}

/// Pins the bug-#4 fix (this commit): browser_close on an open
/// session removes the page so a subsequent action gets a fresh
/// page rather than reusing the stale one. We can't directly
/// observe "fresh vs stale" without invoking another navigate, but
/// we CAN observe that close-then-list reports the session is gone.
#[tokio::test]
#[ignore = "launches real Chrome — opt-in via `cargo test -- --ignored browser_e2e`"]
async fn close_actually_removes_session_page() {
    let mgr = Arc::new(BrowserManager::new());
    let nav = BrowserNavigateTool::new(mgr.clone());
    let close = BrowserCloseTool::new(mgr.clone());
    let ctx = ToolExecutionContext::new(Uuid::new_v4());

    nav.execute(serde_json::json!({ "url": TEST_URL }), &ctx)
        .await
        .expect("navigate must succeed");

    let key = BrowserManager::page_name_for_session(ctx.session_id);
    assert!(
        mgr.list_pages().await.contains(&key),
        "after navigate, session must have an open page"
    );

    let close_res = close.execute(serde_json::json!({}), &ctx).await.unwrap();
    assert!(
        close_res.success,
        "browser_close must succeed on an open session"
    );
    assert!(
        !mgr.list_pages().await.contains(&key),
        "after browser_close, the session's page must be gone from the manager"
    );

    // Idempotent: second close on the same session must still report success.
    let second = close.execute(serde_json::json!({}), &ctx).await.unwrap();
    assert!(
        second.success,
        "browser_close must be idempotent — second call should not error"
    );
}
