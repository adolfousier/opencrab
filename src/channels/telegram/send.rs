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

/// Delete a Telegram message, tolerating failure. Cleanup paths (streaming
/// placeholder teardown, recreate swaps, aborted flows) must never break on
/// a delete Telegram rejects — but silently dropping the error (the old
/// `let _ =` culture) hides real breakage. Warn on anything except
/// "already gone", which is the outcome the caller wanted anyway.
/// Model: `fire_reaction`. (#1085 P3)
pub async fn best_effort_delete<C>(bot: &Bot, chat_id: C, msg_id: MessageId, why: &str)
where
    C: Into<ChatId>,
{
    let chat = chat_id.into();
    if let Err(e) = bot.delete_message(chat, msg_id).await {
        let text = e.to_string();
        let quiet =
            text.contains("message to delete not found") || text.contains("message id is invalid");
        if !quiet {
            // Review F4: the ids are in hand — a delete warn that cannot be
            // correlated to a chat is half a forensics record.
            tracing::warn!(
                "Telegram: best-effort delete failed ({}): chat={} msg={} err={}",
                why,
                chat.0,
                msg_id.0,
                e
            );
        }
    }
}

/// Fire a chat action (typing indicator) and warn if Telegram rejects it.
/// Pure cosmetics on the wire — never breaks a turn — but the failure line
/// tells forensics the bot was mid-turn when the API hiccuped. Awaited
/// sibling of [`chat_action_in_thread`] so call sites stop discarding the
/// Result. (#1085 P3)
pub async fn fire_chat_action<C>(
    bot: &Bot,
    chat_id: C,
    thread_id: Option<ThreadId>,
    action: ChatAction,
    why: &str,
) where
    C: Into<ChatId>,
{
    if let Err(e) = chat_action_in_thread(bot, chat_id, thread_id, action)
        .await
        .map(|_| ())
    {
        tracing::warn!("Telegram: chat action failed ({}): {}", why, e);
    }
}

/// Fire a message without breaking the caller, with correlation telemetry
/// on both exits (#1085 review F10). Single attempt by design: these are
/// system notices (welcomes, cowork setup, agent help replies) — the #297
/// delay-never-drop contract covers command replies, not courtesy pings,
/// and a single attempt can never stall a turn. Model: `best_effort_delete`.
#[allow(clippy::too_many_arguments)]
pub async fn best_effort_note<C>(
    bot: &Bot,
    chat_id: C,
    thread_id: Option<ThreadId>,
    text: &str,
    parse_mode: Option<teloxide::types::ParseMode>,
    origin: &str,
    origin_detail: &str,
    why: &str,
) where
    C: Into<ChatId>,
{
    let chat = chat_id.into();
    let len = text.len();
    let hash8 = super::telemetry::content_hash8(text);
    let request = message_in_thread(bot, chat, thread_id, text);
    let request = match parse_mode {
        Some(mode) => request.parse_mode(mode),
        None => request,
    };
    match request.await {
        Ok(m) => super::telemetry::log_send_success(
            origin,
            origin_detail,
            "-",
            "note",
            why,
            chat.0,
            thread_id.map(|t| t.0.0),
            m.id.0,
            len,
            &hash8,
        ),
        Err(e) => {
            tracing::warn!(
                "Telegram: best-effort note failed ({origin}/{origin_detail} {why}): chat={} err={e}",
                chat.0
            );
        }
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

    // 1. Native rich, as a whole message. `post_rich` owns the telemetry
    // line for this send (with origin + detail threaded through), so the
    // outbox does not double-log the rich success (review F3/F8).
    if super::rich::should_send_native_rich(markdown) {
        match super::rich::send_rich_with_mermaid_id(
            bot.token(),
            chat_id.0,
            thread_id,
            markdown,
            origin,
            origin_detail,
        )
        .await
        {
            Ok(id) => return Ok(vec![(id, markdown.to_string())]),
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
        match super::intermediates::send_html_or_plain(bot, chat_id, thread_id, chunk, origin).await
        {
            Ok(mid) => {
                super::telemetry::log_send_success(
                    origin,
                    origin_detail,
                    "-",
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
