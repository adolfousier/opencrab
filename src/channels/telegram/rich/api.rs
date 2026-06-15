//! Raw Bot API client for `sendRichMessage` (Bot API 10.1, 2026-06).
//!
//! teloxide 0.17 has no binding for this method yet, so we call it directly
//! over HTTP. `InputRichMessage` takes the message as a `markdown` (or `html`)
//! string — Telegram parses it server-side into rich blocks (tables, headings,
//! nested lists, math) — so there is no block JSON to construct: we pass the
//! model's markdown straight through.

use teloxide::Bot;
use teloxide::types::ThreadId;

/// Rich-first send: attempt `sendRichMessage` for `markdown`, pulling the token
/// from `bot`. Returns `Err` on failure so the caller can fall back to its
/// existing (plain or HTML) send path. Default-on — there is no config toggle.
pub(crate) async fn try_send_rich(
    bot: &Bot,
    chat_id: i64,
    thread_id: Option<ThreadId>,
    markdown: &str,
) -> anyhow::Result<()> {
    send_rich_markdown(bot.token(), chat_id, thread_id, markdown).await
}

/// Send `markdown` as a native rich message via `sendRichMessage`.
///
/// Returns `Err` on any transport failure or non-`ok` API response so the
/// caller can fall back to the HTML `parse_mode` path. `thread_id` targets a
/// forum topic when present.
pub(crate) async fn send_rich_markdown(
    token: &str,
    chat_id: i64,
    thread_id: Option<ThreadId>,
    markdown: &str,
) -> anyhow::Result<()> {
    let url = format!("https://api.telegram.org/bot{token}/sendRichMessage");
    let body = build_body(chat_id, thread_id, markdown);

    let resp = reqwest::Client::new().post(&url).json(&body).send().await?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();

    if status.is_success() && parsed.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
        Ok(())
    } else {
        let desc = parsed
            .get("description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(&text);
        anyhow::bail!("sendRichMessage failed ({status}): {desc}")
    }
}

/// Build the `sendRichMessage` JSON request body. Split out so the request
/// shape is unit-testable without a live bot.
pub(crate) fn build_body(
    chat_id: i64,
    thread_id: Option<ThreadId>,
    markdown: &str,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "chat_id": chat_id,
        "rich_message": { "markdown": markdown },
    });
    if let Some(t) = thread_id {
        // ThreadId wraps a MessageId(i32).
        body["message_thread_id"] = serde_json::json!(t.0.0);
    }
    body
}
