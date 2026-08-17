//! Raw Bot API client for `sendRichMessage` (Bot API 10.1, 2026-06).
//!
//! teloxide 0.17 has no binding for this method yet, so we call it directly
//! over HTTP. `InputRichMessage` takes the message as a `markdown` (or `html`)
//! string — Telegram parses it server-side into rich blocks (tables, headings,
//! nested lists, math) — so there is no block JSON to construct: we pass the
//! model's markdown straight through.

use teloxide::types::ThreadId;

/// Send `html` as a native rich message and return the new message id
/// (#420 path A). The HTML input mode is parsed server-side into rich
/// blocks, so `<details><summary>` becomes a native RichBlockDetails
/// collapsible, which the markdown input mode cannot express.
/// `reply_markup` is optional — pass `None` for no keyboard.
pub(crate) async fn send_rich_html_id(
    token: &str,
    chat_id: i64,
    thread_id: Option<ThreadId>,
    html: &str,
    reply_markup: Option<&serde_json::Value>,
) -> anyhow::Result<i32> {
    let url = format!("https://api.telegram.org/bot{token}/sendRichMessage");
    let mut body = build_body_html(chat_id, thread_id, html);
    if let Some(kb) = reply_markup {
        body["reply_markup"] = kb.clone();
    }
    let result = post_rich(&url, &body).await?;
    result
        .get("message_id")
        .and_then(serde_json::Value::as_i64)
        .map(|id| id as i32)
        .ok_or_else(|| anyhow::anyhow!("sendRichMessage ok but response carried no message_id"))
}

/// Edit an existing rich message with HTML input (#420 path A).
/// `reply_markup` is optional — pass `None` to leave the keyboard unchanged.
pub(crate) async fn edit_rich_html(
    token: &str,
    chat_id: i64,
    message_id: i32,
    html: &str,
    reply_markup: Option<&serde_json::Value>,
) -> anyhow::Result<()> {
    let url = format!("https://api.telegram.org/bot{token}/editMessageText");
    let mut body = serde_json::json!({
        "chat_id": chat_id,
        "message_id": message_id,
        "rich_message": { "html": html },
    });
    if let Some(kb) = reply_markup {
        body["reply_markup"] = kb.clone();
    }
    post_and_check(&url, &body).await
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

/// How many times a 429 is waited out before giving up and letting the caller
/// fall back. One retry was not enough: Telegram hands out multi-second waits
/// under load, so a single retry lands inside the same window it was told to
/// wait for and the send is abandoned while still rate limited.
const RICH_MAX_RETRIES: u32 = 3;

/// POST `body` to `url`, treating anything other than `{"ok":true,...}` as an
/// error (surfacing Telegram's `description`). Returns the `result` object.
///
/// A 429 is retried up to `RICH_MAX_RETRIES` times, honouring the server's
/// `retry_after`. The error returned always describes the LAST attempt: the
/// previous version rebound `status`/`text`/`parsed` inside the retry block,
/// so those bindings fell out of scope and a retry that failed for a new
/// reason was reported as the rate limit that preceded it (#927).
/// Extract correlation fields for a rich-API call from the request pair
/// (#1085 P1a): method from the URL, chat/thread from the body, len and
/// hash from the payload text. Works for sends, edits and deletes alike
/// (non-send calls simply log msg=0).
fn rich_send_fields<'a>(
    url: &'a str,
    body: &serde_json::Value,
) -> (&'a str, i64, Option<i32>, usize, String) {
    let method = url.rsplit('/').next().unwrap_or("?");
    let chat_id = body
        .get("chat_id")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    let thread = body
        .get("message_thread_id")
        .and_then(serde_json::Value::as_i64)
        .map(|t| t as i32);
    let text = body
        .pointer("/rich_message/markdown")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            body.pointer("/rich_message/html")
                .and_then(serde_json::Value::as_str)
        })
        .unwrap_or("");
    (
        method,
        chat_id,
        thread,
        text.len(),
        crate::channels::telegram::telemetry::content_hash8(text),
    )
}

async fn post_rich(url: &str, body: &serde_json::Value) -> anyhow::Result<serde_json::Value> {
    let client = reqwest::Client::new();
    let mut attempt = 0u32;

    loop {
        let resp = client.post(url).json(body).send().await?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();

        if status.is_success()
            && parsed.get("ok").and_then(serde_json::Value::as_bool) == Some(true)
        {
            let result = parsed
                .get("result")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            // Correlation telemetry (#1085 P1a): every rich send that lands
            // gets one line with full correlation fields.
            let (method, chat_id, thread, len, hash8) = rich_send_fields(url, body);
            let msg_id = result
                .get("message_id")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0) as i32;
            crate::channels::telegram::telemetry::log_send_success(
                "turn", "rich_api", method, chat_id, thread, msg_id, len, &hash8,
            );
            return Ok(result);
        }

        if status.as_u16() == 429 && attempt < RICH_MAX_RETRIES {
            let retry_after = parsed
                .get("parameters")
                .and_then(|p| p.get("retry_after"))
                .and_then(|r| r.as_u64())
                .unwrap_or(5);
            attempt += 1;
            crate::channels::telegram::rate_limit::wait_out(
                "rich API",
                std::time::Duration::from_secs(retry_after),
                &format!(" (attempt {attempt}/{RICH_MAX_RETRIES})"),
            )
            .await;
            continue;
        }

        // Out of retries, or an error that retrying cannot fix. Report THIS
        // attempt so the caller logs why the send actually failed.
        let desc = parsed
            .get("description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(&text);
        if status.as_u16() == 429 {
            tracing::warn!(
                "Rich API still rate limited after {RICH_MAX_RETRIES} retries — falling back"
            );
        }
        {
            // Correlation telemetry (#1085 P1a): a failed rich send must
            // carry the same fields a successful one does.
            let (method, chat_id, thread, len, hash8) = rich_send_fields(url, body);
            crate::channels::telegram::telemetry::log_send_failure(
                "turn",
                "rich_api",
                method,
                chat_id,
                thread,
                len,
                &hash8,
                &format!("({status}): {desc}"),
            );
        }
        anyhow::bail!("Telegram rich API error ({status}): {desc}")
    }
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

/// Build the `sendRichMessage` body with HTML input (#420 path A).
/// `InputRichMessage` accepts `markdown` or `html`; HTML is the mode that
/// can express RichBlockDetails via `<details><summary>`.
pub(crate) fn build_body_html(
    chat_id: i64,
    thread_id: Option<ThreadId>,
    html: &str,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "chat_id": chat_id,
        "rich_message": { "html": html },
    });
    if let Some(t) = thread_id {
        body["message_thread_id"] = serde_json::json!(t.0.0);
    }
    body
}

/// Send `markdown` with a `media` array as a native rich message
/// (Bot API 10.2+, #1044). The markdown references each image via
/// `tg://photo?id=<id>`; the `media` array maps each id to a renderer URL
/// Telegram fetches server-side. This is the mode that embeds images while
/// keeping pipe tables native. Returns the new message id.
pub(crate) async fn send_rich_markdown_media_id(
    token: &str,
    chat_id: i64,
    thread_id: Option<ThreadId>,
    markdown: &str,
    media: &[super::mermaid::MediaEntry],
) -> anyhow::Result<i32> {
    let url = format!("https://api.telegram.org/bot{token}/sendRichMessage");
    let result = post_rich(
        &url,
        &build_body_markdown_media(chat_id, thread_id, markdown, media),
    )
    .await?;
    result
        .get("message_id")
        .and_then(serde_json::Value::as_i64)
        .map(|id| id as i32)
        .ok_or_else(|| anyhow::anyhow!("sendRichMessage ok but response carried no message_id"))
}

/// Build the `sendRichMessage` body with markdown input + a `media` array
/// (Bot API 10.2+, #1044). Split out so the request shape is unit-testable
/// without a live bot. Matches the validated prototype (message 1073):
/// `rich_message: {markdown, media: [{id, media: {type:"photo", media:url}}]}`.
pub(crate) fn build_body_markdown_media(
    chat_id: i64,
    thread_id: Option<ThreadId>,
    markdown: &str,
    media: &[super::mermaid::MediaEntry],
) -> serde_json::Value {
    let media_arr: Vec<serde_json::Value> = media
        .iter()
        .map(|m| {
            serde_json::json!({
                "id": m.id,
                "media": { "type": "photo", "media": m.url },
            })
        })
        .collect();
    let mut body = serde_json::json!({
        "chat_id": chat_id,
        "rich_message": { "markdown": markdown, "media": media_arr },
    });
    if let Some(t) = thread_id {
        body["message_thread_id"] = serde_json::json!(t.0.0);
    }
    body
}
