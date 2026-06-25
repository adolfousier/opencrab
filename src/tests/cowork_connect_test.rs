//! Tests for the `cowork_connect` tool — the agent-callable launcher that lets
//! `/cowork` run from the TUI (or any channel): it builds the Telegram deep
//! link + QR and registers the session in the live TelegramState.

use crate::brain::tools::cowork_connect::CoworkConnectTool;
use crate::brain::tools::{Tool, ToolExecutionContext};
use crate::channels::telegram::TelegramState;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn without_bot_username_errors_clearly() {
    // No bot username known → Telegram isn't connected; can't build a link.
    let tool = CoworkConnectTool::new(Arc::new(TelegramState::new()));
    let ctx = ToolExecutionContext::new(Uuid::new_v4());
    let r = tool.execute(serde_json::json!({}), &ctx).await.unwrap();
    assert!(!r.success);
    assert!(r.error.unwrap().to_lowercase().contains("telegram"));
}

#[tokio::test]
async fn builds_deep_link_with_bot_and_session() {
    let state = Arc::new(TelegramState::new());
    state.set_bot_username("testcrabbot".to_string()).await;
    let tool = CoworkConnectTool::new(state.clone());
    let ctx = ToolExecutionContext::new(Uuid::new_v4());

    let r = tool.execute(serde_json::json!({}), &ctx).await.unwrap();
    assert!(r.success, "{:?}", r.error);
    assert!(
        r.output.contains("t.me/testcrabbot?startgroup=cowork_"),
        "deep link missing: {}",
        r.output
    );
}
