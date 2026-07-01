//! Tests for caption and reply_parameters support in `send_photo` / `send_document`
//! actions of the telegram_send tool (#257).
//!
//! Since the actual Telegram Bot API calls require a live connection, these tests
//! verify the schema contract and the graceful error path when the bot is not
//! connected. The parameter-extraction patterns are also exercised directly.

use crate::brain::tools::telegram_send::TelegramSendTool;
use crate::brain::tools::r#trait::Tool;
use crate::brain::tools::r#trait::ToolExecutionContext;
use crate::channels::telegram::TelegramState;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

/// Helper: build a `TelegramSendTool` with a disconnected state.
fn make_tool() -> TelegramSendTool {
    let state = Arc::new(TelegramState::new());
    TelegramSendTool::new(state)
}

// ── Schema contract tests ──────────────────────────────────────────────

#[test]
fn schema_has_caption_property() {
    let tool = make_tool();
    let schema = tool.input_schema();
    let caption = schema.pointer("/properties/caption");
    assert!(
        caption.is_some(),
        "input_schema must include a 'caption' property"
    );
    let desc = caption
        .unwrap()
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        desc.contains("send_photo") && desc.contains("send_document"),
        "caption description should mention send_photo and send_document, got: {desc}"
    );
}

#[test]
fn schema_message_id_mentions_media_actions() {
    let tool = make_tool();
    let schema = tool.input_schema();
    let desc = schema
        .pointer("/properties/message_id/description")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        desc.contains("send_photo") && desc.contains("send_document"),
        "message_id description should mention send_photo/send_document, got: {desc}"
    );
}

// ── Graceful error path (no bot connected) ─────────────────────────────

#[tokio::test]
async fn send_photo_without_bot_returns_error() {
    let tool = make_tool();
    let ctx = ToolExecutionContext::new(Uuid::new_v4());
    let result = tool
        .execute(
            json!({"action": "send_photo", "photo_url": "https://example.com/cat.png"}),
            &ctx,
        )
        .await
        .unwrap();
    assert!(!result.success, "should fail when bot not connected");
    let err = result.error.unwrap_or_default();
    assert!(
        err.contains("not connected"),
        "error should mention not connected, got: {err}"
    );
}

#[tokio::test]
async fn send_document_without_bot_returns_error() {
    let tool = make_tool();
    let ctx = ToolExecutionContext::new(Uuid::new_v4());
    let result = tool
        .execute(
            json!({"action": "send_document", "document_url": "https://example.com/doc.pdf"}),
            &ctx,
        )
        .await
        .unwrap();
    assert!(!result.success, "should fail when bot not connected");
    let err = result.error.unwrap_or_default();
    assert!(
        err.contains("not connected"),
        "error should mention not connected, got: {err}"
    );
}

#[tokio::test]
async fn send_photo_with_caption_without_bot_returns_error() {
    let tool = make_tool();
    let ctx = ToolExecutionContext::new(Uuid::new_v4());
    let result = tool
        .execute(
            json!({
                "action": "send_photo",
                "photo_url": "https://example.com/cat.png",
                "caption": "A cute cat"
            }),
            &ctx,
        )
        .await
        .unwrap();
    assert!(!result.success, "should fail when bot not connected");
}

#[tokio::test]
async fn send_document_with_caption_and_reply_without_bot_returns_error() {
    let tool = make_tool();
    let ctx = ToolExecutionContext::new(Uuid::new_v4());
    let result = tool
        .execute(
            json!({
                "action": "send_document",
                "document_url": "/tmp/report.pdf",
                "caption": "Monthly report",
                "message_id": 42
            }),
            &ctx,
        )
        .await
        .unwrap();
    assert!(!result.success, "should fail when bot not connected");
}

// ── Parameter extraction pattern tests ─────────────────────────────────

#[test]
fn caption_extraction_from_json() {
    // Mirrors the extraction pattern in send_photo/send_document:
    //   input.get("caption").and_then(|v| v.as_str())
    let input = json!({"caption": "Hello world"});
    let caption = input.get("caption").and_then(|v| v.as_str());
    assert_eq!(caption, Some("Hello world"));

    let input_no_caption = json!({});
    assert_eq!(
        input_no_caption.get("caption").and_then(|v| v.as_str()),
        None
    );

    let input_null = json!({"caption": null});
    assert_eq!(input_null.get("caption").and_then(|v| v.as_str()), None);

    let input_number = json!({"caption": 123});
    assert_eq!(
        input_number.get("caption").and_then(|v| v.as_str()),
        None,
        "non-string caption should be ignored"
    );
}

#[test]
fn message_id_extraction_from_json() {
    // Mirrors: input.get("message_id").and_then(|v| v.as_i64())
    let input = json!({"message_id": 42});
    let mid = input.get("message_id").and_then(|v| v.as_i64());
    assert_eq!(mid, Some(42));

    let input_missing = json!({});
    assert_eq!(
        input_missing.get("message_id").and_then(|v| v.as_i64()),
        None
    );

    let input_string = json!({"message_id": "42"});
    assert_eq!(
        input_string.get("message_id").and_then(|v| v.as_i64()),
        None,
        "string message_id should be ignored (must be integer)"
    );
}

#[test]
fn empty_caption_string_is_treated_as_present() {
    // An empty string is still Some("") from as_str(). The Telegram API
    // accepts 0-1024 chars, so an empty caption is valid (removes previous
    // caption on forwarded messages). The code does NOT skip empty captions.
    let input = json!({"caption": ""});
    let caption = input.get("caption").and_then(|v| v.as_str());
    assert_eq!(caption, Some(""));
}
