//! Thread-aware Telegram send helpers.
//!
//! Wraps teloxide's `bot.send_message` / `send_photo` / `send_chat_action`
//! constructors with an `Option<ThreadId>` parameter so forum-topic replies
//! land in the originating topic instead of the group's General chat
//! (issue #130).
//!
//! Each helper returns the underlying teloxide request type, so existing
//! chains (`.parse_mode()`, `.reply_markup()`, `.reply_to_message_id()`,
//! `.await`) continue to work unchanged. The only call-site delta is the
//! function name + an extra `thread_id` argument.
//!
//! `thread_id = None` is a no-op — the helper produces the same request
//! you'd get from `bot.send_message(chat_id, text)` directly. Safe to use
//! everywhere even in non-topic chats.

use teloxide::Bot;
use teloxide::payloads::ForwardMessageSetters;
use teloxide::payloads::SendChatActionSetters;
use teloxide::payloads::SendDocumentSetters;
use teloxide::payloads::SendLocationSetters;
use teloxide::payloads::SendMessageSetters;
use teloxide::payloads::SendPhotoSetters;
use teloxide::payloads::SendPollSetters;
use teloxide::prelude::Requester;
use teloxide::requests::JsonRequest;
use teloxide::types::{ChatAction, ChatId, InputFile, MessageId, ThreadId};

/// Look up the thread_id of the most recent Telegram message stored for
/// `chat_id` in `channel_messages`. Returns `None` when no row exists,
/// when the row's thread_id is `NULL` (regular non-topic chat), or when
/// the stored value can't be parsed as an `i32`. Used by proactive send
/// paths (`telegram_send` tool, startup resume in cli/ui.rs) that have
/// no incoming `Message` to read `thread_id` from.
///
/// Reads via `crate::db::global_pool()` because the proactive surfaces
/// don't carry a `Pool` through their call chain. Returns `None` if the
/// global pool hasn't been initialized yet (early startup, tests).
pub async fn latest_thread_id_for_chat(chat_id: i64) -> Option<ThreadId> {
    let pool = crate::db::global_pool()?;
    let repo = crate::db::ChannelMessageRepository::new(pool.clone());
    let chat_id_str = chat_id.to_string();
    let rows = repo
        .recent(Some("telegram"), &chat_id_str, 1, None, None)
        .await
        .ok()?;
    let row = rows.into_iter().next()?;
    let tid_str = row.thread_id?;
    tid_str.parse::<i32>().ok().map(|n| ThreadId(MessageId(n)))
}

/// `bot.send_message(chat_id, text)` with optional `message_thread_id`.
/// Returns the teloxide request so callers can chain `.parse_mode()`,
/// `.reply_markup()`, etc. before `.await`.
pub fn message_in_thread<C, T>(
    bot: &Bot,
    chat_id: C,
    thread_id: Option<ThreadId>,
    text: T,
) -> JsonRequest<teloxide::payloads::SendMessage>
where
    C: Into<ChatId>,
    T: Into<String>,
{
    let req = bot.send_message(chat_id.into(), text.into());
    match thread_id {
        Some(t) => req.message_thread_id(t),
        None => req,
    }
}

/// `bot.send_photo(chat_id, photo)` with optional `message_thread_id`.
pub fn photo_in_thread<C>(
    bot: &Bot,
    chat_id: C,
    thread_id: Option<ThreadId>,
    photo: InputFile,
) -> teloxide::requests::MultipartRequest<teloxide::payloads::SendPhoto>
where
    C: Into<ChatId>,
{
    let req = bot.send_photo(chat_id.into(), photo);
    match thread_id {
        Some(t) => req.message_thread_id(t),
        None => req,
    }
}

/// `bot.send_document(chat_id, document)` with optional `message_thread_id`.
/// Completes the `*_in_thread` family for #1079: documents landed in General
/// in forum groups because the tool arm built its own request.
pub fn document_in_thread<C>(
    bot: &Bot,
    chat_id: C,
    thread_id: Option<ThreadId>,
    document: InputFile,
) -> teloxide::requests::MultipartRequest<teloxide::payloads::SendDocument>
where
    C: Into<ChatId>,
{
    let req = bot.send_document(chat_id.into(), document);
    match thread_id {
        Some(t) => req.message_thread_id(t),
        None => req,
    }
}

/// `bot.send_location(chat_id, lat, lng)` with optional `message_thread_id` (#1079).
pub fn location_in_thread<C>(
    bot: &Bot,
    chat_id: C,
    thread_id: Option<ThreadId>,
    latitude: f64,
    longitude: f64,
) -> JsonRequest<teloxide::payloads::SendLocation>
where
    C: Into<ChatId>,
{
    let req = bot.send_location(chat_id.into(), latitude, longitude);
    match thread_id {
        Some(t) => req.message_thread_id(t),
        None => req,
    }
}

/// `bot.send_poll(chat_id, question, options)` with optional `message_thread_id` (#1079).
pub fn poll_in_thread<C>(
    bot: &Bot,
    chat_id: C,
    thread_id: Option<ThreadId>,
    question: String,
    options: Vec<teloxide::types::InputPollOption>,
) -> JsonRequest<teloxide::payloads::SendPoll>
where
    C: Into<ChatId>,
{
    let req = bot.send_poll(chat_id.into(), question, options);
    match thread_id {
        Some(t) => req.message_thread_id(t),
        None => req,
    }
}

/// `bot.forward_message(to_chat, from_chat, message_id)` with optional
/// `message_thread_id` (#1079): forwards landed in General in forum groups.
pub fn forward_in_thread<C>(
    bot: &Bot,
    to_chat_id: C,
    from_chat_id: ChatId,
    message_id: MessageId,
    thread_id: Option<ThreadId>,
) -> JsonRequest<teloxide::payloads::ForwardMessage>
where
    C: Into<ChatId>,
{
    let req = bot.forward_message(to_chat_id.into(), from_chat_id, message_id);
    match thread_id {
        Some(t) => req.message_thread_id(t),
        None => req,
    }
}

/// `bot.send_chat_action(chat_id, action)` with optional `message_thread_id`.
/// The "typing" indicator goes to the right topic instead of the General
/// chat — important for forum groups where the bot is mentioned across
/// multiple topics.
pub fn chat_action_in_thread<C>(
    bot: &Bot,
    chat_id: C,
    thread_id: Option<ThreadId>,
    action: ChatAction,
) -> JsonRequest<teloxide::payloads::SendChatAction>
where
    C: Into<ChatId>,
{
    let req = bot.send_chat_action(chat_id.into(), action);
    match thread_id {
        Some(t) => req.message_thread_id(t),
        None => req,
    }
}

/// One send ladder for every proactive Telegram writer (#1085 P1b R2).
///
/// Owns the wire path end to end: rich-gate (whole message, never chunked —
/// a split table breaks) → `markdown_to_telegram_html` → 4096 chunks →
/// [`send_html_or_plain`] (which retries 429s per #297 and falls back to
/// plain text when Telegram rejects the markup). Callers keep their
/// delivery *decisions*; this function owns retry, thread routing,
/// fallback and telemetry so they stop being per-writer choices. This is
/// the deliberate Q4 behavior change from the #1085 grill: cron and the
/// telegram_send tool previously had NO plain-text fallback and would 400
/// on markup Telegram rejects — now they inherit it.
///
/// `origin`/`origin_detail` feed the correlation telemetry (cron → job
/// name, tool → arm name). Returns `(message_id, content)` pairs for
/// reply-recovery persistence. Errors describe the failing attempt and
/// name any chunks already delivered.
pub(crate) async fn send_markdown_outbox(
    bot: &Bot,
    chat_id: ChatId,
    thread_id: Option<ThreadId>,
    markdown: &str,
    origin: &str,
    origin_detail: &str,
) -> std::result::Result<Vec<(i32, String)>, String> {
    let thread = thread_id.map(|t| t.0.0);
    let hash8 = super::telemetry::content_hash8(markdown);
    let len = markdown.len();

    // 1. Native rich, as a whole message.
    if super::rich::should_send_native_rich(markdown) {
        match super::rich::send_rich_with_mermaid_id(bot.token(), chat_id.0, thread_id, markdown)
            .await
        {
            Ok(id) => {
                super::telemetry::log_send_success(
                    origin, "outbox", "rich", chat_id.0, thread, id, len, &hash8,
                );
                return Ok(vec![(id, markdown.to_string())]);
            }
            Err(e) => {
                tracing::warn!(
                    "{origin}/{origin_detail}: native rich send failed ({e}) — falling back to HTML"
                );
            }
        }
    }

    // 2. Universal HTML ladder, chunked to Telegram's limit.
    let html = super::handler::markdown_to_telegram_html(markdown);
    let chunks = super::handler::split_message(&html, 4096);
    let total = chunks.len();
    let mut sent: Vec<(i32, String)> = Vec::new();
    for (i, chunk) in chunks.into_iter().enumerate() {
        match super::intermediates::send_html_or_plain(bot, chat_id, thread_id, chunk).await {
            Ok(mid) => {
                super::telemetry::log_send_success(
                    origin,
                    "outbox",
                    "html_chunk",
                    chat_id.0,
                    thread,
                    mid.0,
                    chunk.len(),
                    &super::telemetry::content_hash8(chunk),
                );
                sent.push((mid.0, chunk.to_string()));
            }
            Err(e) => {
                let partial = if sent.is_empty() {
                    String::new()
                } else {
                    format!(" ({} of {total} chunks already delivered)", sent.len())
                };
                return Err(format!(
                    "{origin}/{origin_detail} chunk {}/{total} failed{partial}: {e}",
                    i + 1
                ));
            }
        }
    }
    Ok(sent)
}

/// Persist delivered outbox messages for reply recovery (#234, #1085 P1b
/// R2). One implementation of what cron's `deliver_telegram` and the tool's
/// `persist_outgoing` previously built separately as byte-identical rows.
/// `thread_id` is stamped when known (cron rows previously lost it).
pub(crate) async fn record_outgoing(
    pool: Option<crate::db::Pool>,
    chat_id: i64,
    thread_id: Option<ThreadId>,
    sent: &[(i32, String)],
) {
    if sent.is_empty() {
        return;
    }
    let Some(pool) = pool.or_else(|| crate::db::global_pool().cloned()) else {
        tracing::warn!("telegram outbox: no DB pool — outgoing messages not persisted");
        return;
    };
    let repo = crate::db::ChannelMessageRepository::new(pool);
    let chat_id_str = chat_id.to_string();
    let thread = thread_id.map(|t| t.0.0.to_string());
    for (mid, content) in sent {
        if content.trim().is_empty() {
            continue;
        }
        let cm = crate::db::models::ChannelMessage::new(
            "telegram".to_string(),
            chat_id_str.clone(),
            None,
            "bot:opencrabs".to_string(),
            "OpenCrabs".to_string(),
            content.clone(),
            "text".to_string(),
            Some(mid.to_string()),
        )
        .with_thread(thread.clone(), None);
        if let Err(e) = repo.insert(&cm).await {
            tracing::warn!(
                "telegram outbox: failed to persist message {mid} for reply-recovery: {e}"
            );
        }
    }
}
