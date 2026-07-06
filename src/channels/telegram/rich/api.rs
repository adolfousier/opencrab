//! Raw Bot API client for `sendRichMessage` (Bot API 10.1, 2026-06).
//!
//! teloxide 0.17 has no binding for this method yet, so we call it directly
//! over HTTP. `InputRichMessage` takes the message as a `markdown` (or `html`)
//! string — Telegram parses it server-side into rich blocks (tables, headings,
//! nested lists, math) — so there is no block JSON to construct: we pass the
//! model's markdown straight through.

use teloxide::types::ThreadId;

/// Send an ephemeral rich-message draft (DM-only, auto-expires in ~30s).
///
/// Drafts are private to the chat and produce no persistent notification.
/// Re-sending with the same `draft_id` causes Telegram to animate the
/// transition instead of creating a new message. Call this every ~25s to
/// keep the draft alive while the agent is working.
///
/// Only works in private chats — Telegram silently ignores drafts in groups.
pub(crate) async fn send_rich_message_draft(
    token: &str,
    chat_id: i64,
    draft_id: i32,
    markdown: &str,
) -> anyhow::Result<i32> {
    let url = format!("https://api.telegram.org/bot{token}/sendRichMessageDraft");
    let body = serde_json::json!({
        "chat_id": chat_id,
        "draft_id": draft_id,
        "rich_message": { "markdown": markdown },
    });
    let result = post_rich(&url, &body).await?;
    result
        .get("message_id")
        .and_then(serde_json::Value::as_i64)
        .map(|id| id as i32)
        .ok_or_else(|| {
            anyhow::anyhow!("sendRichMessageDraft ok but response carried no message_id")
        })
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
    post_and_check(&url, &build_body(chat_id, thread_id, markdown)).await
}

/// Send `markdown` as a native rich message and return the new message id.
/// Used for intermediate streamed segments, which must be tracked for later
/// footer-append / dedup. Returns `Err` so the caller can fall back to HTML.
pub(crate) async fn send_rich_markdown_id(
    token: &str,
    chat_id: i64,
    thread_id: Option<ThreadId>,
    markdown: &str,
) -> anyhow::Result<i32> {
    let url = format!("https://api.telegram.org/bot{token}/sendRichMessage");
    let result = post_rich(&url, &build_body(chat_id, thread_id, markdown)).await?;
    result
        .get("message_id")
        .and_then(serde_json::Value::as_i64)
        .map(|id| id as i32)
        .ok_or_else(|| anyhow::anyhow!("sendRichMessage ok but response carried no message_id"))
}

/// Edit an existing message with rich markdown via `editMessageText` + `rich_message`.
/// Used to append footers to intermediate rich messages without downgrading to HTML.
/// Returns `Err` so the caller can fall back to the HTML edit path.
pub(crate) async fn edit_rich_markdown(
    token: &str,
    chat_id: i64,
    message_id: i32,
    markdown: &str,
) -> anyhow::Result<()> {
    let url = format!("https://api.telegram.org/bot{token}/editMessageText");
    let body = serde_json::json!({
        "chat_id": chat_id,
        "message_id": message_id,
        "rich_message": { "markdown": markdown },
    });
    post_and_check(&url, &body).await
}

/// POST `body` to `url`, treating anything other than `{"ok":true,...}` as an
/// error (surfacing Telegram's `description`). Returns the `result` object.
/// Handles 429 RetryAfter with a single retry.
async fn post_rich(url: &str, body: &serde_json::Value) -> anyhow::Result<serde_json::Value> {
    let resp = reqwest::Client::new().post(url).json(body).send().await?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();

    if status.is_success() && parsed.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
        return Ok(parsed
            .get("result")
            .cloned()
            .unwrap_or(serde_json::Value::Null));
    }

    // Handle 429 rate limit with RetryAfter
    if status.as_u16() == 429 {
        let retry_after = parsed
            .get("parameters")
            .and_then(|p| p.get("retry_after"))
            .and_then(|r| r.as_u64())
            .unwrap_or(5);
        tracing::warn!("Rich API rate limited, retrying after {retry_after}s");
        tokio::time::sleep(std::time::Duration::from_secs(retry_after)).await;

        // Single retry
        let resp = reqwest::Client::new().post(url).json(body).send().await?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();

        if status.is_success()
            && parsed.get("ok").and_then(serde_json::Value::as_bool) == Some(true)
        {
            return Ok(parsed
                .get("result")
                .cloned()
                .unwrap_or(serde_json::Value::Null));
        }
    }

    let desc = parsed
        .get("description")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(&text);
    anyhow::bail!("Telegram rich API error ({status}): {desc}")
}

/// POST `body` and discard the result — for calls where only success matters.
async fn post_and_check(url: &str, body: &serde_json::Value) -> anyhow::Result<()> {
    post_rich(url, body).await.map(|_| ())
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
