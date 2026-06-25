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
use teloxide::payloads::SendChatActionSetters;
use teloxide::payloads::SendMessageSetters;
use teloxide::payloads::SendPhotoSetters;
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
    let rows = repo.recent(Some("telegram"), &chat_id_str, 1, None).await.ok()?;
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
