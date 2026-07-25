//! Ephemeral group replies: Bot API 10.2 (`receiver_user_id`), 2026-07-14.
//!
//! Telegram can deliver a group message to a single member: nobody else in
//! the chat ever sees it. Every OpenCrabs slash command is owner-gated, so
//! today the whole group watches replies addressed to one person. Scoping
//! those to the invoker keeps the group context and drops the noise (#756).
//!
//! teloxide 0.17 / teloxide-core 0.13 has no binding for the parameter, so
//! this calls `sendMessage` directly over HTTP, mirroring [`super::rich::api`].
//! `sendRichMessage` did *not* gain `receiver_user_id` in 10.2, so an
//! ephemeral reply is always plain or HTML, never a native rich message.
//!
//! Nothing here returns an error. A send that does not land reports how much
//! was delivered and the caller finishes the job on its normal public path,
//! which is exactly the pre-10.2 behaviour. That covers both a Bot API server
//! older than 10.2 and a future rename of the parameter.

use teloxide::Bot;
use teloxide::types::{ChatId, ThreadId};

/// The user an ephemeral reply should be scoped to, or `None` when the reply
/// must stay an ordinary message.
///
/// A DM is already private and has no other members to hide the reply from,
/// so `receiver_user_id` buys nothing there, and asking for it would trade a
/// working plain send for an untested one.
pub(crate) fn receiver_for(is_dm: bool, user_id: i64) -> Option<i64> {
    if is_dm { None } else { Some(user_id) }
}

/// Build the `sendMessage` body for an ephemeral reply. Split out from the
/// transport so the request shape is unit-testable without a live bot.
///
/// `parse_html` mirrors the caller's existing choice: command output that
/// went through `command_md_to_html` needs `parse_mode`, bare status acks
/// must not have it or their `<`/`&` would be read as markup.
pub(crate) fn build_body(
    chat_id: i64,
    thread_id: Option<ThreadId>,
    receiver_user_id: i64,
    text: &str,
    parse_html: bool,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "chat_id": chat_id,
        "text": text,
        "receiver_user_id": receiver_user_id,
    });
    if parse_html {
        body["parse_mode"] = serde_json::json!("HTML");
    }
    if let Some(t) = thread_id {
        // ThreadId wraps a MessageId(i32).
        body["message_thread_id"] = serde_json::json!(t.0.0);
    }
    body
}

/// Deliver one ephemeral reply. `true` when it landed; `false` means the
/// caller must send `text` publicly as it did before 10.2.
pub(crate) async fn send_one(
    token: &str,
    chat_id: i64,
    thread_id: Option<ThreadId>,
    receiver_user_id: i64,
    text: &str,
    parse_html: bool,
) -> bool {
    let body = build_body(chat_id, thread_id, receiver_user_id, text, parse_html);
    post_send(token, &body).await
}

/// Deliver `chunks` in order, scoped to `receiver_user_id`.
///
/// Returns how many *leading* chunks landed, so the caller can finish the
/// message publicly without duplicating what already arrived: `0` means fall
/// back wholesale, a short count means public-send `chunks[n..]`. Stops at the
/// first failure rather than interleaving delivered and dropped chunks, which
/// would reorder a split reply.
pub(crate) async fn send_html_chunks(
    token: &str,
    chat_id: i64,
    thread_id: Option<ThreadId>,
    receiver_user_id: i64,
    chunks: &[&str],
) -> usize {
    let mut delivered = 0usize;
    for chunk in chunks {
        if !send_one(token, chat_id, thread_id, receiver_user_id, chunk, true).await {
            break;
        }
        delivered += 1;
    }
    delivered
}

/// Send a one-off command ack: scoped to `receiver` when there is one, and
/// on the ordinary rate-limit-retrying public path otherwise.
///
/// `receiver` comes from [`receiver_for`], so `None` (a DM) skips straight to
/// the public send. Acks are plain text: they carry no markup and must not be
/// parsed as HTML.
pub(crate) async fn send_ack(
    bot: &Bot,
    chat_id: ChatId,
    thread_id: Option<ThreadId>,
    receiver: Option<i64>,
    text: &str,
) -> std::result::Result<(), teloxide::RequestError> {
    if let Some(rx) = receiver
        && send_one(bot.token(), chat_id.0, thread_id, rx, text, false).await
    {
        return Ok(());
    }
    super::intermediates::send_retrying_rate_limit("command reply", || {
        super::send::message_in_thread(bot, chat_id, thread_id, text)
    })
    .await
    .map(|_| ())
}

/// POST an ephemeral `sendMessage`, logging why it did not land.
///
/// A 400 here is the expected shape of "this server predates 10.2" or "the
/// bot may not scope messages in this chat", so it is a warning and the
/// caller recovers, but it is never swallowed, because a silent failure
/// looks identical to the feature working while every reply stays public.
async fn post_send(token: &str, body: &serde_json::Value) -> bool {
    let url = format!("https://api.telegram.org/bot{token}/sendMessage");
    let resp = match reqwest::Client::new().post(&url).json(body).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(
                "Telegram: ephemeral sendMessage transport error: {e}, sending publicly"
            );
            return false;
        }
    };
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();

    if status.is_success() && parsed.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
        return true;
    }

    let desc = parsed
        .get("description")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(&text);
    tracing::warn!("Telegram: ephemeral sendMessage rejected ({status}): {desc}, sending publicly");
    false
}
