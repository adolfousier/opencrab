//! Telegram Send Tool
//!
//! Agent-callable tool for full Telegram control: send, reply, edit, delete,
//! pin/unpin, forward, media, polls, inline buttons, chat info, moderation,
//! and reactions. Always prefer this tool over http_request — credentials
//! are handled securely.

use super::error::Result;
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolHints, ToolResult};
use crate::channels::telegram::TelegramState;
use crate::channels::telegram::intermediates::send_retrying_rate_limit;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use teloxide::payloads::SendDocumentSetters;
use teloxide::payloads::SendMessageSetters;
use teloxide::payloads::SendPhotoSetters;
use teloxide::prelude::*;
use teloxide::types::{
    ChatId, InlineKeyboardButton, InlineKeyboardMarkup, InputFile, MessageId, ReactionType,
    ReplyParameters, UserId,
};
use uuid::Uuid;

/// Tool for comprehensive Telegram bot control (19 actions).
pub struct TelegramSendTool {
    telegram_state: Arc<TelegramState>,
}

impl TelegramSendTool {
    pub fn new(telegram_state: Arc<TelegramState>) -> Self {
        Self { telegram_state }
    }
}

/// Extract a required non-empty string param, returning ToolResult::error on failure.
#[allow(clippy::result_large_err)]
fn get_str<'a>(input: &'a Value, key: &str) -> std::result::Result<&'a str, ToolResult> {
    match input.get(key).and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => Ok(s),
        _ => Err(ToolResult::error(format!(
            "Missing required parameter '{key}'."
        ))),
    }
}

/// Parse a required integer param as i64.
#[allow(clippy::result_large_err)]
fn get_id(input: &Value, key: &str) -> std::result::Result<i64, ToolResult> {
    match input.get(key).and_then(|v| v.as_i64()) {
        Some(id) => Ok(id),
        None => Err(ToolResult::error(format!(
            "Missing required parameter '{key}' (must be an integer)."
        ))),
    }
}

/// Resolve a media reference (`photo_url` / `document_url`) into a Telegram
/// `InputFile`. An HTTP(S) URL is handed to Telegram as-is (it fetches the
/// remote file). Anything else is treated as a local path: the file is read
/// into memory and uploaded directly, the same way the channel handler sends
/// generated images and voice notes. Without this, local paths were passed to
/// `InputFile::url()` and rejected as invalid URLs (#181).
#[allow(clippy::result_large_err)]
pub(crate) async fn resolve_input_file(
    reference: &str,
    label: &str,
) -> std::result::Result<InputFile, ToolResult> {
    if reference.starts_with("http://") || reference.starts_with("https://") {
        return reference
            .parse()
            .map(InputFile::url)
            .map_err(|e| ToolResult::error(format!("Invalid {label}: {e}")));
    }

    // Local file: read bytes and upload from memory.
    let path = crate::brain::tools::error::expand_tilde(reference);
    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "file".to_string());
            Ok(InputFile::memory(bytes).file_name(name))
        }
        Err(e) => Err(ToolResult::error(format!(
            "Failed to read local {label} '{}': {e}",
            path.display()
        ))),
    }
}

/// Resolve chat_id: explicit param or owner fallback.
#[allow(clippy::result_large_err)]
/// Resolve a forum-topic `thread_id` for a proactive Telegram send.
///
/// Precedence:
///   1. Explicit `thread_id` field in the tool input — the agent
///      asked for a specific topic, honour it. Lets cron jobs / the
///      agent route messages to a topic OTHER than the most recent
///      one (e.g. "post the release notes in #announcements even
///      though the last message came from #dev").
///   2. Session origin topic via `session_topic(session_id)` — the
///      forum topic this interaction started in, the same in-memory
///      map `follow_up_question` routes through (#450). Makes replies
///      land back in the originating topic with no explicit routing.
///   3. Auto-lookup via `latest_thread_id_for_chat(chat_id)` — the
///      fallback that closed #130, picking up the most recently
///      stored topic so non-forum chats and routine replies still
///      land in the right place without the agent having to know.
///
/// Returns `None` when no path produces a value (non-forum chat, no
/// session origin, empty channel history, explicit value outside i32 range).
pub(crate) async fn resolve_thread_id(
    input: &Value,
    chat_id: i64,
    session_id: Uuid,
    state: &TelegramState,
) -> Option<teloxide::types::ThreadId> {
    if let Some(tid) = input.get("thread_id").and_then(|v| v.as_i64())
        && let Ok(tid_i32) = i32::try_from(tid)
    {
        return Some(teloxide::types::ThreadId(teloxide::types::MessageId(
            tid_i32,
        )));
    }
    // Session origin topic — the forum topic this interaction started in, the
    // same in-memory map follow_up_question routes through (#450). This is why
    // a reply sent from a topic lands back in that topic without the agent
    // passing thread_id. Cold/cron sessions have no entry, so this is skipped.
    if let Some(tid) = state.session_topic(session_id).await {
        return Some(teloxide::types::ThreadId(teloxide::types::MessageId(tid)));
    }
    crate::channels::telegram::send::latest_thread_id_for_chat(chat_id).await
}

/// Persist outgoing bot messages to `channel_messages` keyed by their Telegram
/// `message_id`, so a later reply can recover their text by id — exactly like
/// the normal reply path does. Without this, a user replying to a message the
/// bot posted proactively (a report, a cron post, any `telegram_send`) hits an
/// empty `channel_messages` lookup, and because rich/cron messages arrive with
/// no readable text in the reply, the agent can only honestly say it cannot see
/// it. `sent` is `(message_id, content)` pairs (one per chunk for plain sends).
async fn persist_outgoing(
    chat_id: i64,
    thread_id: Option<teloxide::types::ThreadId>,
    sent: &[(i32, String)],
) {
    if sent.is_empty() {
        return;
    }
    let Some(pool) = crate::db::global_pool() else {
        tracing::warn!("telegram_send: no global DB pool — outgoing message not persisted");
        return;
    };
    let repo = crate::db::ChannelMessageRepository::new(pool.clone());
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
                "telegram_send: failed to persist outgoing message {mid} for reply-recovery: {e}"
            );
        }
    }
}

async fn chat_or_err(
    input: &Value,
    state: &TelegramState,
    session_id: Uuid,
) -> std::result::Result<i64, ToolResult> {
    if let Some(id) = input.get("chat_id").and_then(|v| v.as_i64()) {
        return Ok(id);
    }
    // Session origin chat — where this interaction started, same map
    // follow_up_question uses (#450). Falls through to owner_chat_id for
    // cold/cron sessions that never bound a chat.
    if let Some(id) = state.session_chat(session_id).await {
        return Ok(id);
    }
    match state.owner_chat_id().await {
        Some(id) => Ok(id),
        None => Err(ToolResult::error(
            "No owner chat ID known yet and no 'chat_id' parameter provided. \
             The owner needs to send at least one message to the bot first, \
             or specify a chat_id."
                .to_string(),
        )),
    }
}

// Macro to early-return Ok(err_result) when a param helper returns Err.
macro_rules! pget {
    ($expr:expr) => {
        match $expr {
            Ok(v) => v,
            Err(e) => return Ok(e),
        }
    };
}

#[async_trait]
impl Tool for TelegramSendTool {
    fn name(&self) -> &str {
        "telegram_send"
    }

    fn description(&self) -> &str {
        "Full Telegram control: send messages, reply, edit, delete, pin/unpin, forward, \
         send photos/documents/locations/polls, inline buttons, get chat info, list admins, \
         check member count/status, ban/unban users, and set emoji reactions. \
         Always use telegram_send instead of http_request — credentials handled securely. \
         Requires Telegram to be connected first."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": [
                        "send", "reply", "edit", "delete", "pin", "unpin",
                        "forward", "send_photo", "send_document", "send_location",
                        "send_poll", "send_buttons", "get_chat",
                        "get_chat_administrators", "get_chat_member_count", "get_chat_member",
                        "ban_user", "unban_user", "set_reaction", "list_topics"
                    ],
                    "description": "The Telegram action to perform. \
                        `list_topics` returns the (thread_id, topic_name) pairs the bot has \
                        observed in a forum-enabled supergroup — use this to translate a \
                        user-typed topic name like \"#announcements\" to the numeric thread_id \
                        you then pass to `send` / `reply` / `send_photo` via the `thread_id` field."
                },
                "message": {
                    "type": "string",
                    "description": "Message text (send, reply, edit, send_buttons)"
                },
                "chat_id": {
                    "type": "integer",
                    "description": "Telegram chat ID. Omit to use owner's chat."
                },
                "thread_id": {
                    "type": "integer",
                    "description": "Optional forum-topic ID for groups with topics enabled. Omit to auto-route to the most recent topic seen in the chat (the usual case for replies to ongoing conversations). Pass an explicit value to route to a DIFFERENT topic — e.g. post a release announcement in #announcements when the latest message came from #dev. Ignored for non-forum chats."
                },
                "caption": {
                    "type": "string",
                    "description": "Caption for send_photo / send_document (0-1024 chars). Attaches text context to the media."
                },
                "message_id": {
                    "type": "integer",
                    "description": "Target message ID for reply/edit/delete/pin/unpin/forward/set_reaction, or the message to reply to when used with send_photo/send_document"
                },
                "from_chat_id": {
                    "type": "integer",
                    "description": "Source chat ID for forward action"
                },
                "photo_url": {
                    "type": "string",
                    "description": "Photo for send_photo: an HTTPS URL or a local file path (e.g. /tmp/chart.png or ~/.opencrabs/out.png)"
                },
                "document_url": {
                    "type": "string",
                    "description": "Document for send_document: an HTTPS URL or a local file path (e.g. /tmp/report.pdf or ~/.opencrabs/data.csv)"
                },
                "latitude": {
                    "type": "number",
                    "description": "Latitude for send_location"
                },
                "longitude": {
                    "type": "number",
                    "description": "Longitude for send_location"
                },
                "poll_question": {
                    "type": "string",
                    "description": "Poll question text for send_poll"
                },
                "poll_options": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Array of poll option strings (2–10) for send_poll"
                },
                "buttons": {
                    "type": "array",
                    "items": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "text": {"type": "string"},
                                "callback_data": {"type": "string"}
                            }
                        }
                    },
                    "description": "2D array of button rows for send_buttons. Each button has 'text' and 'callback_data'."
                },
                "user_id": {
                    "type": "integer",
                    "description": "Telegram user ID for ban_user/unban_user"
                },
                "emoji": {
                    "type": "string",
                    "description": "Emoji for set_reaction (e.g. \"👍\")"
                }
            },
            "required": ["action"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::Network]
    }

    fn hints(&self) -> ToolHints {
        ToolHints {
            read_only: false,
            destructive: true,
            idempotent: false,
            open_world: true,
        }
    }

    async fn execute(&self, input: Value, context: &ToolExecutionContext) -> Result<ToolResult> {
        let action = match input.get("action").and_then(|v| v.as_str()) {
            Some(a) if !a.is_empty() => a.to_string(),
            _ => {
                return Ok(ToolResult::error(
                    "Missing required 'action' parameter.".to_string(),
                ));
            }
        };

        let bot = match self.telegram_state.bot().await {
            Some(b) => b,
            None => {
                return Ok(ToolResult::error(
                    "Telegram is not connected. Ask the user to connect Telegram first \
                     (use the telegram_connect tool)."
                        .to_string(),
                ));
            }
        };

        match action.as_str() {
            // ── send ─────────────────────────────────────────────────────────
            "send" => {
                let text = pget!(get_str(&input, "message")).to_string();
                let chat_id =
                    pget!(chat_or_err(&input, &self.telegram_state, context.session_id).await);
                // Explicit `thread_id` wins; auto-lookup is the
                // fallback for the common case (#130).
                let thread_id =
                    resolve_thread_id(&input, chat_id, context.session_id, &self.telegram_state)
                        .await;
                // Structured messages (tables, headings, lists, math) go through
                // the native rich path as a whole — never chunked, since a split
                // table would break. Plain prose, and any rich failure, fall back
                // to the chunked plain-text send so a message is never dropped
                // and Telegram's parser never reinterprets incidental characters.
                // Track sent (message_id, content) so the message is persisted
                // for reply-recovery below.
                let mut sent: Vec<(i32, String)> = Vec::new();
                // Convert markdown to Telegram HTML upfront so that both the
                // rich path and the plain fallback send properly formatted
                // content. The rich API's HTML input mode renders tags natively
                // (tables, bold, code); the markdown input mode showed literal
                // tags as text when the model emitted HTML (#834).
                // Same rich mode as the agent's own replies (#871). This used
                // send_rich_html_id, whose HTML input mode returns 200 and then
                // renders headings inline and FLATTENS tables into a run-on
                // paragraph — a silent wrong render with nothing logged, which
                // is worse than an error. Rich-markdown renders tables
                // correctly and is what every delivered rich message actually
                // goes through, so both paths now behave identically.
                let sent_rich = crate::channels::telegram::rich::should_send_native_rich(&text)
                    && match crate::channels::telegram::rich::send_rich_with_mermaid_id(
                        bot.token(),
                        chat_id,
                        thread_id,
                        &text,
                    )
                    .await
                    {
                        Ok(id) => {
                            sent.push((id, text.clone()));
                            true
                        }
                        Err(e) => {
                            tracing::warn!("telegram_send: rich send failed, sending plain: {e}");
                            false
                        }
                    };
                if !sent_rich {
                    // Convert only on the fallback now that the rich send takes
                    // markdown directly (#871); split so chunks stay within
                    // Telegram's 4096 limit.
                    let html = crate::channels::telegram::handler::markdown_to_telegram_html(&text);
                    for chunk in crate::channels::telegram::handler::split_message(&html, 4096) {
                        let chunk_str = chunk.to_string();
                        // Retry Telegram 429 (RetryAfter): a rate-limited send
                        // must be delayed and retried, not dropped as a failure
                        // to the agent (#524).
                        let result = send_retrying_rate_limit("telegram_send send", || {
                            crate::channels::telegram::send::message_in_thread(
                                &bot,
                                ChatId(chat_id),
                                thread_id,
                                chunk_str.clone(),
                            )
                            .parse_mode(teloxide::types::ParseMode::Html)
                        })
                        .await;
                        match result {
                            Ok(m) => sent.push((m.id.0, chunk_str)),
                            Err(e) => {
                                return Ok(ToolResult::error(format!("Failed to send: {e}")));
                            }
                        }
                    }
                }
                // Persist so a later reply to this message can be read back by id
                // (a report/cron post replied-to would otherwise be unrecoverable).
                persist_outgoing(chat_id, thread_id, &sent).await;
                Ok(ToolResult::success(format!(
                    "Message sent to chat {chat_id}."
                )))
            }

            // ── reply ────────────────────────────────────────────────────────
            "reply" => {
                let text = pget!(get_str(&input, "message")).to_string();
                let chat_id =
                    pget!(chat_or_err(&input, &self.telegram_state, context.session_id).await);
                let message_id = pget!(get_id(&input, "message_id"));
                let thread_id =
                    resolve_thread_id(&input, chat_id, context.session_id, &self.telegram_state)
                        .await;
                // Convert markdown to Telegram HTML, same as the "send"
                // action, so formatting (bold, code, tables) renders instead
                // of arriving as raw literal tags (#834).
                let html = crate::channels::telegram::handler::markdown_to_telegram_html(&text);
                let reply_text = text.clone();
                match send_retrying_rate_limit("telegram_send reply", || {
                    crate::channels::telegram::send::message_in_thread(
                        &bot,
                        ChatId(chat_id),
                        thread_id,
                        html.clone(),
                    )
                    .parse_mode(teloxide::types::ParseMode::Html)
                    .reply_parameters(ReplyParameters::new(MessageId(message_id as i32)))
                })
                .await
                {
                    Ok(m) => {
                        // Persist for reply-recovery (a user can reply to this bot reply).
                        persist_outgoing(chat_id, thread_id, &[(m.id.0, reply_text)]).await;
                        Ok(ToolResult::success(format!(
                            "Reply sent to message {message_id}."
                        )))
                    }
                    Err(e) => Ok(ToolResult::error(format!("Failed to reply: {e}"))),
                }
            }

            // ── edit ─────────────────────────────────────────────────────────
            "edit" => {
                let text = pget!(get_str(&input, "message")).to_string();
                let chat_id =
                    pget!(chat_or_err(&input, &self.telegram_state, context.session_id).await);
                let message_id = pget!(get_id(&input, "message_id"));
                // Convert markdown to Telegram HTML, same as the "send"
                // action, so formatting renders correctly (#834).
                let html = crate::channels::telegram::handler::markdown_to_telegram_html(&text);
                match send_retrying_rate_limit("telegram_send edit", || {
                    bot.edit_message_text(
                        ChatId(chat_id),
                        MessageId(message_id as i32),
                        html.clone(),
                    )
                    .parse_mode(teloxide::types::ParseMode::Html)
                })
                .await
                {
                    Ok(_) => Ok(ToolResult::success(format!("Message {message_id} edited."))),
                    Err(e) => Ok(ToolResult::error(format!("Failed to edit: {e}"))),
                }
            }

            // ── delete ───────────────────────────────────────────────────────
            "delete" => {
                let chat_id =
                    pget!(chat_or_err(&input, &self.telegram_state, context.session_id).await);
                let message_id = pget!(get_id(&input, "message_id"));
                match send_retrying_rate_limit("telegram_send delete", || {
                    bot.delete_message(ChatId(chat_id), MessageId(message_id as i32))
                })
                .await
                {
                    Ok(_) => Ok(ToolResult::success(format!(
                        "Message {message_id} deleted."
                    ))),
                    Err(e) => Ok(ToolResult::error(format!("Failed to delete: {e}"))),
                }
            }

            // ── pin ──────────────────────────────────────────────────────────
            "pin" => {
                let chat_id =
                    pget!(chat_or_err(&input, &self.telegram_state, context.session_id).await);
                let message_id = pget!(get_id(&input, "message_id"));
                match send_retrying_rate_limit("telegram_send pin", || {
                    bot.pin_chat_message(ChatId(chat_id), MessageId(message_id as i32))
                })
                .await
                {
                    Ok(_) => Ok(ToolResult::success(format!("Message {message_id} pinned."))),
                    Err(e) => Ok(ToolResult::error(format!("Failed to pin: {e}"))),
                }
            }

            // ── unpin ────────────────────────────────────────────────────────
            "unpin" => {
                let chat_id =
                    pget!(chat_or_err(&input, &self.telegram_state, context.session_id).await);
                match send_retrying_rate_limit("telegram_send unpin", || {
                    bot.unpin_chat_message(ChatId(chat_id))
                })
                .await
                {
                    Ok(_) => Ok(ToolResult::success(
                        "Latest pinned message unpinned.".to_string(),
                    )),
                    Err(e) => Ok(ToolResult::error(format!("Failed to unpin: {e}"))),
                }
            }

            // ── forward ──────────────────────────────────────────────────────
            "forward" => {
                let to_chat =
                    pget!(chat_or_err(&input, &self.telegram_state, context.session_id).await);
                let from_chat = pget!(get_id(&input, "from_chat_id"));
                let message_id = pget!(get_id(&input, "message_id"));
                // Route into the destination topic (#1079): forwards used to
                // bypass thread resolution entirely and land in General.
                let thread_id =
                    resolve_thread_id(&input, to_chat, context.session_id, &self.telegram_state)
                        .await;
                match send_retrying_rate_limit("telegram_send forward", || {
                    crate::channels::telegram::send::forward_in_thread(
                        &bot,
                        ChatId(to_chat),
                        ChatId(from_chat),
                        MessageId(message_id as i32),
                        thread_id,
                    )
                })
                .await
                {
                    Ok(_) => Ok(ToolResult::success(format!(
                        "Message {message_id} forwarded from chat {from_chat} to {to_chat}."
                    ))),
                    Err(e) => Ok(ToolResult::error(format!("Failed to forward: {e}"))),
                }
            }

            // ── send_photo ───────────────────────────────────────────────────
            "send_photo" => {
                let chat_id =
                    pget!(chat_or_err(&input, &self.telegram_state, context.session_id).await);
                let reference = pget!(get_str(&input, "photo_url")).to_string();
                let caption = input
                    .get("caption")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                // Collapse an identical photo+caption re-sent to the same chat
                // within the dedup window (#721) — model repeats or post-timeout
                // retries otherwise land the same media twice back-to-back.
                if !self.telegram_state.claim_media_send(
                    "send_photo",
                    chat_id,
                    &reference,
                    caption.as_deref(),
                ) {
                    tracing::info!(
                        "telegram_send: suppressed duplicate send_photo to chat {chat_id} ({reference})"
                    );
                    return Ok(ToolResult::success(format!(
                        "Photo already sent to chat {chat_id} moments ago — skipped the duplicate."
                    )));
                }
                let file = pget!(resolve_input_file(&reference, "photo_url").await);
                let reply_to = input.get("message_id").and_then(|v| v.as_i64());
                // Route into the topic (#1079): photos landed in General in
                // forum groups because this arm built its own request instead
                // of the shared photo_in_thread helper.
                let thread_id =
                    resolve_thread_id(&input, chat_id, context.session_id, &self.telegram_state)
                        .await;
                match send_retrying_rate_limit("telegram_send send_photo", || {
                    let mut req = crate::channels::telegram::send::photo_in_thread(
                        &bot,
                        ChatId(chat_id),
                        thread_id,
                        file.clone(),
                    );
                    if let Some(ref c) = caption {
                        req = req.caption(c.clone());
                    }
                    if let Some(mid) = reply_to {
                        req = req.reply_parameters(ReplyParameters::new(MessageId(mid as i32)));
                    }
                    req
                })
                .await
                {
                    Ok(_) => Ok(ToolResult::success(format!(
                        "Photo sent to chat {chat_id}."
                    ))),
                    Err(e) => Ok(ToolResult::error(format!("Failed to send photo: {e}"))),
                }
            }

            // ── send_document ────────────────────────────────────────────────
            "send_document" => {
                let chat_id =
                    pget!(chat_or_err(&input, &self.telegram_state, context.session_id).await);
                let reference = pget!(get_str(&input, "document_url")).to_string();
                let caption = input
                    .get("caption")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                // Collapse an identical document+caption re-sent to the same
                // chat within the dedup window (#721) — a large upload that
                // times out client-side after Telegram already delivered it,
                // or a model repeat, otherwise lands the same file twice.
                if !self.telegram_state.claim_media_send(
                    "send_document",
                    chat_id,
                    &reference,
                    caption.as_deref(),
                ) {
                    tracing::info!(
                        "telegram_send: suppressed duplicate send_document to chat {chat_id} ({reference})"
                    );
                    return Ok(ToolResult::success(format!(
                        "Document already sent to chat {chat_id} moments ago — skipped the duplicate."
                    )));
                }
                let file = pget!(resolve_input_file(&reference, "document_url").await);
                let reply_to = input.get("message_id").and_then(|v| v.as_i64());
                // Route into the topic (#1079): documents landed in General.
                let thread_id =
                    resolve_thread_id(&input, chat_id, context.session_id, &self.telegram_state)
                        .await;
                match send_retrying_rate_limit("telegram_send send_document", || {
                    let mut req = crate::channels::telegram::send::document_in_thread(
                        &bot,
                        ChatId(chat_id),
                        thread_id,
                        file.clone(),
                    );
                    if let Some(ref c) = caption {
                        req = req.caption(c.clone());
                    }
                    if let Some(mid) = reply_to {
                        req = req.reply_parameters(ReplyParameters::new(MessageId(mid as i32)));
                    }
                    req
                })
                .await
                {
                    Ok(_) => Ok(ToolResult::success(format!(
                        "Document sent to chat {chat_id}."
                    ))),
                    Err(e) => Ok(ToolResult::error(format!("Failed to send document: {e}"))),
                }
            }

            // ── send_location ────────────────────────────────────────────────
            "send_location" => {
                let chat_id =
                    pget!(chat_or_err(&input, &self.telegram_state, context.session_id).await);
                let lat = match input.get("latitude").and_then(|v| v.as_f64()) {
                    Some(v) => v,
                    None => {
                        return Ok(ToolResult::error(
                            "Missing required 'latitude' parameter.".to_string(),
                        ));
                    }
                };
                let lng = match input.get("longitude").and_then(|v| v.as_f64()) {
                    Some(v) => v,
                    None => {
                        return Ok(ToolResult::error(
                            "Missing required 'longitude' parameter.".to_string(),
                        ));
                    }
                };
                // Route into the topic (#1079): locations landed in General.
                let thread_id =
                    resolve_thread_id(&input, chat_id, context.session_id, &self.telegram_state)
                        .await;
                match send_retrying_rate_limit("telegram_send send_location", || {
                    crate::channels::telegram::send::location_in_thread(
                        &bot,
                        ChatId(chat_id),
                        thread_id,
                        lat,
                        lng,
                    )
                })
                .await
                {
                    Ok(_) => Ok(ToolResult::success(format!(
                        "Location ({lat}, {lng}) sent to chat {chat_id}."
                    ))),
                    Err(e) => Ok(ToolResult::error(format!("Failed to send location: {e}"))),
                }
            }

            // ── send_poll ────────────────────────────────────────────────────
            "send_poll" => {
                let chat_id =
                    pget!(chat_or_err(&input, &self.telegram_state, context.session_id).await);
                let question = pget!(get_str(&input, "poll_question")).to_string();
                let opts: Vec<String> = match input.get("poll_options").and_then(|v| v.as_array()) {
                    Some(arr) => arr
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect(),
                    None => {
                        return Ok(ToolResult::error(
                            "Missing required 'poll_options' parameter.".to_string(),
                        ));
                    }
                };
                if opts.len() < 2 {
                    return Ok(ToolResult::error(
                        "'poll_options' must have at least 2 options.".to_string(),
                    ));
                }
                let poll_opts: Vec<teloxide::types::InputPollOption> =
                    opts.into_iter().map(|s| s.into()).collect();
                // Route into the topic (#1079): polls landed in General.
                let thread_id =
                    resolve_thread_id(&input, chat_id, context.session_id, &self.telegram_state)
                        .await;
                match send_retrying_rate_limit("telegram_send send_poll", || {
                    crate::channels::telegram::send::poll_in_thread(
                        &bot,
                        ChatId(chat_id),
                        thread_id,
                        question.clone(),
                        poll_opts.clone(),
                    )
                })
                .await
                {
                    Ok(_) => Ok(ToolResult::success(format!("Poll sent to chat {chat_id}."))),
                    Err(e) => Ok(ToolResult::error(format!("Failed to send poll: {e}"))),
                }
            }

            // ── send_buttons ─────────────────────────────────────────────────
            "send_buttons" => {
                let text = pget!(get_str(&input, "message")).to_string();
                let chat_id =
                    pget!(chat_or_err(&input, &self.telegram_state, context.session_id).await);
                // Collect callback_data strings for origin tracking (#878)
                let mut origin_keys: Vec<String> = Vec::new();
                let rows: Vec<Vec<InlineKeyboardButton>> =
                    match input.get("buttons").and_then(|v| v.as_array()) {
                        Some(outer) => outer
                            .iter()
                            .filter_map(|row| row.as_array())
                            .map(|row| {
                                row.iter()
                                    .filter_map(|btn| {
                                        let text =
                                            btn.get("text").and_then(|v| v.as_str())?.to_string();
                                        let data = btn
                                            .get("callback_data")
                                            .and_then(|v| v.as_str())?
                                            .to_string();
                                        origin_keys.push(data.clone());
                                        Some(InlineKeyboardButton::callback(text, data))
                                    })
                                    .collect()
                            })
                            .collect(),
                        None => {
                            return Ok(ToolResult::error(
                                "Missing required 'buttons' parameter.".to_string(),
                            ));
                        }
                    };
                // Register callback_data → session_id so the callback
                // dispatcher routes taps to THIS session (#878).
                self.telegram_state
                    .register_callback_origins(context.session_id, origin_keys);
                let keyboard = InlineKeyboardMarkup::new(rows);
                let html = crate::channels::telegram::handler::markdown_to_telegram_html(&text);
                // Route into the topic (#1079): button messages landed in General.
                let thread_id =
                    resolve_thread_id(&input, chat_id, context.session_id, &self.telegram_state)
                        .await;
                match send_retrying_rate_limit("telegram_send send_buttons", || {
                    crate::channels::telegram::send::message_in_thread(
                        &bot,
                        ChatId(chat_id),
                        thread_id,
                        html.clone(),
                    )
                    .parse_mode(teloxide::types::ParseMode::Html)
                    .reply_markup(keyboard.clone())
                })
                .await
                {
                    Ok(_) => Ok(ToolResult::success(format!(
                        "Message with buttons sent to chat {chat_id}."
                    ))),
                    Err(e) => Ok(ToolResult::error(format!(
                        "Failed to send message with buttons: {e}"
                    ))),
                }
            }

            // ── get_chat ─────────────────────────────────────────────────────
            "get_chat" => {
                let chat_id =
                    pget!(chat_or_err(&input, &self.telegram_state, context.session_id).await);
                match bot.get_chat(ChatId(chat_id)).await {
                    Ok(chat) => {
                        let info = format!(
                            "Chat {}: type={:?}, title={:?}",
                            chat.id,
                            chat.kind,
                            chat.title()
                        );
                        Ok(ToolResult::success(info))
                    }
                    Err(e) => Ok(ToolResult::error(format!("Failed to get chat: {e}"))),
                }
            }

            // ── get_chat_administrators ────────────────────────────────────
            "get_chat_administrators" => {
                let chat_id =
                    pget!(chat_or_err(&input, &self.telegram_state, context.session_id).await);
                match bot.get_chat_administrators(ChatId(chat_id)).await {
                    Ok(admins) => {
                        let lines: Vec<String> = admins
                            .iter()
                            .map(|m| {
                                let u = &m.user;
                                let role = match m.kind {
                                    teloxide::types::ChatMemberKind::Owner { .. } => "owner",
                                    teloxide::types::ChatMemberKind::Administrator { .. } => {
                                        "admin"
                                    }
                                    _ => "member",
                                };
                                let handle = u
                                    .username
                                    .as_ref()
                                    .map(|h| format!(" @{h}"))
                                    .unwrap_or_default();
                                format!("- {} (id={}){} [{}]", u.first_name, u.id, handle, role)
                            })
                            .collect();
                        Ok(ToolResult::success(format!(
                            "Chat {} administrators ({}):\n{}",
                            chat_id,
                            admins.len(),
                            lines.join("\n")
                        )))
                    }
                    Err(e) => Ok(ToolResult::error(format!(
                        "Failed to get administrators: {e}"
                    ))),
                }
            }

            // ── get_chat_member_count ─────────────────────────────────────────
            "get_chat_member_count" => {
                let chat_id =
                    pget!(chat_or_err(&input, &self.telegram_state, context.session_id).await);
                match bot.get_chat_member_count(ChatId(chat_id)).await {
                    Ok(count) => Ok(ToolResult::success(format!(
                        "Chat {chat_id} has {count} members."
                    ))),
                    Err(e) => Ok(ToolResult::error(format!(
                        "Failed to get member count: {e}"
                    ))),
                }
            }

            // ── get_chat_member ───────────────────────────────────────────────
            "get_chat_member" => {
                let chat_id =
                    pget!(chat_or_err(&input, &self.telegram_state, context.session_id).await);
                let uid = pget!(get_id(&input, "user_id"));
                match bot
                    .get_chat_member(ChatId(chat_id), UserId(uid as u64))
                    .await
                {
                    Ok(member) => {
                        let u = &member.user;
                        let status = match member.kind {
                            teloxide::types::ChatMemberKind::Owner { .. } => "owner",
                            teloxide::types::ChatMemberKind::Administrator { .. } => {
                                "administrator"
                            }
                            teloxide::types::ChatMemberKind::Member(_) => "member",
                            teloxide::types::ChatMemberKind::Restricted { .. } => "restricted",
                            teloxide::types::ChatMemberKind::Left => "left",
                            teloxide::types::ChatMemberKind::Banned { .. } => "banned",
                        };
                        let handle = u
                            .username
                            .as_ref()
                            .map(|h| format!(" @{h}"))
                            .unwrap_or_default();
                        Ok(ToolResult::success(format!(
                            "User {} (id={}){}: status={}",
                            u.first_name, u.id, handle, status
                        )))
                    }
                    Err(e) => Ok(ToolResult::error(format!("Failed to get chat member: {e}"))),
                }
            }

            // ── ban_user ─────────────────────────────────────────────────────
            "ban_user" => {
                let chat_id =
                    pget!(chat_or_err(&input, &self.telegram_state, context.session_id).await);
                let user_id = pget!(get_id(&input, "user_id"));
                match send_retrying_rate_limit("telegram_send ban_user", || {
                    bot.ban_chat_member(ChatId(chat_id), UserId(user_id as u64))
                })
                .await
                {
                    Ok(_) => Ok(ToolResult::success(format!(
                        "User {user_id} banned from chat {chat_id}."
                    ))),
                    Err(e) => Ok(ToolResult::error(format!("Failed to ban user: {e}"))),
                }
            }

            // ── unban_user ───────────────────────────────────────────────────
            "unban_user" => {
                let chat_id =
                    pget!(chat_or_err(&input, &self.telegram_state, context.session_id).await);
                let user_id = pget!(get_id(&input, "user_id"));
                match send_retrying_rate_limit("telegram_send unban_user", || {
                    bot.unban_chat_member(ChatId(chat_id), UserId(user_id as u64))
                })
                .await
                {
                    Ok(_) => Ok(ToolResult::success(format!(
                        "User {user_id} unbanned from chat {chat_id}."
                    ))),
                    Err(e) => Ok(ToolResult::error(format!("Failed to unban user: {e}"))),
                }
            }

            // ── set_reaction ─────────────────────────────────────────────────
            "set_reaction" => {
                let chat_id =
                    pget!(chat_or_err(&input, &self.telegram_state, context.session_id).await);
                let message_id = pget!(get_id(&input, "message_id"));
                let emoji = pget!(get_str(&input, "emoji")).to_string();
                let reactions = vec![ReactionType::Emoji {
                    emoji: emoji.clone(),
                }];
                match send_retrying_rate_limit("telegram_send set_reaction", || {
                    bot.set_message_reaction(ChatId(chat_id), MessageId(message_id as i32))
                        .reaction(reactions.clone())
                })
                .await
                {
                    Ok(_) => Ok(ToolResult::success(format!(
                        "Reaction {emoji} set on message {message_id}."
                    ))),
                    Err(e) => Ok(ToolResult::error(format!("Failed to set reaction: {e}"))),
                }
            }

            // ── list_topics ──────────────────────────────────────────────────
            "list_topics" => {
                let chat_id =
                    pget!(chat_or_err(&input, &self.telegram_state, context.session_id).await);
                let Some(pool) = crate::db::global_pool() else {
                    return Ok(ToolResult::error(
                        "Channel message store unavailable (DB not initialised).".to_string(),
                    ));
                };
                let repo = crate::db::ChannelMessageRepository::new(pool.clone());
                let chat_id_str = chat_id.to_string();
                let topics = match repo.topics_for_chat("telegram", &chat_id_str).await {
                    Ok(t) => t,
                    Err(e) => {
                        return Ok(ToolResult::error(format!("Failed to list topics: {e}")));
                    }
                };
                if topics.is_empty() {
                    return Ok(ToolResult::success(format!(
                        "No forum topics observed yet for chat {chat_id}. \
                         Telegram's Bot API has no listForumTopics endpoint — the bot only \
                         learns topic names from messages it sees. Ask a user to post once in \
                         each topic so the bot can capture their names, then retry."
                    )));
                }
                // Render a compact human/agent-readable table.
                let mut out = format!(
                    "Topics in chat {chat_id} (only those the bot has seen activity in):\n"
                );
                out.push_str("  thread_id | topic_name              | messages | last_seen\n");
                for t in &topics {
                    let name = t.topic_name.as_deref().unwrap_or("(unknown)");
                    // Convert epoch seconds (the schema's storage
                    // format for created_at) to a human-readable
                    // UTC timestamp so the agent and any user
                    // reading the output don't have to decode.
                    let last_seen = chrono::DateTime::from_timestamp(t.last_message_at, 0)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
                        .unwrap_or_else(|| t.last_message_at.to_string());
                    out.push_str(&format!(
                        "  {:<9} | {:<23} | {:>8} | {}\n",
                        t.thread_id,
                        name.chars().take(23).collect::<String>(),
                        t.message_count,
                        last_seen,
                    ));
                }
                out.push_str(
                    "\nPass the thread_id back into `send` / `reply` / `send_photo` etc. \
                     via the optional `thread_id` field to route a message into a specific topic.",
                );
                Ok(ToolResult::success(out))
            }

            unknown => Ok(ToolResult::error(format!(
                "Unknown action '{unknown}'. Valid actions: send, reply, edit, delete, pin, \
                 unpin, forward, send_photo, send_document, send_location, send_poll, \
                 send_buttons, get_chat, get_chat_administrators, get_chat_member_count, \
                 get_chat_member, ban_user, unban_user, set_reaction, list_topics"
            ))),
        }
    }
}
