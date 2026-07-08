//! Intermediate-message delivery: standalone narration posts (rich-first
//! with HTML fallback), footer append to the last intermediate, the
//! rate-limit-retrying send wrapper and the HTML-or-plain send.
//!
//! Moved VERBATIM out of handler.rs (#471 phase 1, pure decomposition —
//! only visibility widened to pub(crate) so the handler glob re-export
//! keeps every existing call site and test import stable).

use super::handler::{DisplayItem, StreamingState};
use super::markdown::{markdown_to_telegram_html, split_message, strip_html_tags};
use super::send::message_in_thread;
use crate::utils::sanitize::redact_secrets;
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::{MessageId, ParseMode};

pub(crate) async fn flush_intermediates(
    bot: &Bot,
    chat: ChatId,
    thread_id: Option<teloxide::types::ThreadId>,
    streaming: &Arc<std::sync::Mutex<StreamingState>>,
) {
    let pending: Vec<DisplayItem> = {
        let mut s = streaming.lock().unwrap_or_else(|e| e.into_inner());
        s.display_queue
            .drain(..)
            .filter(|item| matches!(item, DisplayItem::Intermediate(_)))
            .collect()
    };
    for item in pending {
        if let DisplayItem::Intermediate(text) = item {
            let text = crate::utils::sanitize::strip_llm_artifacts(&text);
            let text = redact_secrets(&text);
            let (text, _img_paths) = crate::utils::extract_img_markers(&text);
            // Strip <<react:emoji>> too; see edit-loop site in handle_message.
            // flush_intermediates has no inbound user message in scope (called
            // from resume + follow-up paths), so the directive is stripped but
            // no reaction fires here (#261).
            let (text, _react_emoji) = crate::utils::extract_react_marker(&text);
            {
                let s = streaming.lock().unwrap_or_else(|e| e.into_inner());
                if s.sent_intermediates.iter().any(|prev| prev == &text) {
                    continue;
                }
            }
            if let Some(id) = try_send_intermediate_rich(bot, chat, thread_id, &text).await {
                let mut s = streaming.lock().unwrap_or_else(|e| e.into_inner());
                s.sent_intermediates.push(text.clone());
                s.intermediate_msg_ids.push(id);
                continue;
            }
            let html = markdown_to_telegram_html(&text);
            if html.is_empty() {
                continue;
            }
            let chunks: Vec<String> = split_message(&html, 4096)
                .into_iter()
                .map(|s| s.to_string())
                .collect();
            let mut sent_ids: Vec<MessageId> = Vec::new();
            let mut all_ok = true;
            for chunk in &chunks {
                match send_html_or_plain(bot, chat, thread_id, chunk).await {
                    Ok(id) => sent_ids.push(id),
                    Err(e) => {
                        tracing::warn!(
                            "Telegram: flush intermediate send failed ({e}), leaving for edit loop"
                        );
                        all_ok = false;
                        break;
                    }
                }
            }
            if all_ok {
                let mut s = streaming.lock().unwrap_or_else(|e| e.into_inner());
                s.sent_intermediates.push(text.clone());
                s.intermediate_msg_ids.extend(sent_ids);
            }
        }
    }
}

/// Send an HTML message, falling back to plain text if Telegram rejects the HTML.
/// Returns the resulting `MessageId` so callers that need to track or later delete
/// the message (e.g. intermediate cleanup on cancellation) can do so.
/// Build the edited message body for appending the ctx/tok-s footer to the
/// last intermediate message.
///
/// Used when a turn's final response text deduped to empty because all of
/// it was already delivered as intermediate messages (the common tool-
/// using case). Rather than drop the footer (which left the user never
/// seeing ctx budget on Telegram — 2026-06-06) or send a standalone
/// footer bubble (removed in 7a0ca1c9), we edit the last intermediate
/// message to carry the footer inline.
///
/// Reconstructs the last chunk exactly as it was originally sent
/// (`markdown_to_telegram_html` + `split_message(_, 4096)` then `.last()`),
/// appends the footer, and returns `None` when:
/// - the footer or intermediate text is empty, OR
/// - the combined result would exceed Telegram's 4096-char cap (never
///   truncate real content to make room for metadata).
///
/// Pure + free function so the fit/reconstruct logic is unit-testable
/// without a live bot.
pub(crate) fn build_last_intermediate_with_footer(
    last_intermediate_text: &str,
    footer: &str,
) -> Option<String> {
    if footer.is_empty() || last_intermediate_text.is_empty() {
        return None;
    }
    let html = markdown_to_telegram_html(last_intermediate_text);
    let chunks = split_message(&html, 4096);
    let last_chunk = chunks.last()?;
    let combined = format!("{last_chunk}\n\n{footer}");
    if combined.chars().count() > 4096 {
        None
    } else {
        Some(combined)
    }
}

/// Append a footer to the last intermediate message, preserving rich format
/// when the intermediate was sent as a native rich message. Editing with
/// ParseMode::Html would downgrade rich tables to card view — this helper
/// tries the rich edit first and falls back to HTML only on failure.
pub(crate) async fn append_footer_to_last_intermediate(
    bot: &Bot,
    chat_id: ChatId,
    inter_id: MessageId,
    inter_text: &str,
    footer: &str,
) {
    if super::rich::should_send_native_rich(inter_text) {
        let rich_md = format!("{inter_text}\n\n{footer}");
        match super::rich::api::edit_rich_markdown(bot.token(), chat_id.0, inter_id.0, &rich_md)
            .await
        {
            Ok(()) => return,
            Err(e) => {
                tracing::warn!("Telegram: rich footer edit failed, falling back to HTML ({e})");
            }
        }
    }
    if let Some(edited) = build_last_intermediate_with_footer(inter_text, footer)
        && let Err(e) = bot
            .edit_message_text(chat_id, inter_id, &edited)
            .parse_mode(ParseMode::Html)
            .await
    {
        tracing::warn!("Telegram: failed to append ctx footer to last intermediate ({e})");
    }
}

/// Send a structured intermediate segment as a native rich message, returning
/// its id for tracking. Returns `None` when the text carries no rich structure
/// or the rich API rejects it — the caller then falls back to the HTML path.
pub(crate) async fn try_send_intermediate_rich(
    bot: &Bot,
    chat_id: ChatId,
    thread_id: Option<teloxide::types::ThreadId>,
    text: &str,
) -> Option<MessageId> {
    if !super::rich::should_send_native_rich(text) {
        return None;
    }
    match super::rich::api::send_rich_markdown_id(bot.token(), chat_id.0, thread_id, text).await {
        Ok(id) => Some(MessageId(id)),
        Err(e) => {
            tracing::warn!("Telegram: intermediate rich send failed, using HTML: {e}");
            None
        }
    }
}

/// Run a Telegram send, waiting out `RetryAfter` (429) up to 3 attempts.
///
/// Command replies are programmatic: a per-chat rate limit (typically a
/// streaming turn editing its placeholder into the same chat) must DELAY
/// them, never drop them. The command branches used a bare `.await?`, so
/// the 429 propagated out of the handler and the reply vanished with a
/// single error log line — /models looked "stuck" while a turn streamed
/// and worked right after it completed (#297). Non-429 errors and
/// exhausted retries still propagate to the caller.
pub(crate) async fn send_retrying_rate_limit<T, F, Fut>(
    what: &str,
    mut send: F,
) -> std::result::Result<T, teloxide::RequestError>
where
    F: FnMut() -> Fut,
    Fut: std::future::IntoFuture<Output = std::result::Result<T, teloxide::RequestError>>,
{
    const MAX_RETRIES: u32 = 3;
    let mut attempt = 0u32;
    loop {
        match send().await {
            Err(teloxide::RequestError::RetryAfter(secs)) if attempt < MAX_RETRIES => {
                attempt += 1;
                tracing::warn!(
                    "Telegram: {what} rate-limited, waiting {}s before retry ({attempt}/{MAX_RETRIES})",
                    secs.seconds()
                );
                tokio::time::sleep(secs.duration()).await;
            }
            Err(teloxide::RequestError::RetryAfter(secs)) => {
                tracing::error!(
                    "Telegram: {what} still rate-limited after {MAX_RETRIES} retries ({}s) — giving up",
                    secs.seconds()
                );
                return Err(teloxide::RequestError::RetryAfter(secs));
            }
            other => return other,
        }
    }
}

pub(crate) async fn send_html_or_plain(
    bot: &Bot,
    chat_id: ChatId,
    thread_id: Option<teloxide::types::ThreadId>,
    html: &str,
) -> std::result::Result<MessageId, teloxide::RequestError> {
    match message_in_thread(bot, chat_id, thread_id, html)
        .parse_mode(ParseMode::Html)
        .await
    {
        Ok(m) => Ok(m.id),
        Err(teloxide::RequestError::RetryAfter(secs)) => {
            tracing::warn!(
                "Telegram: HTML send rate-limited, waiting {}s before retry",
                secs.seconds()
            );
            tokio::time::sleep(secs.duration()).await;
            // Retry as HTML after waiting
            match message_in_thread(bot, chat_id, thread_id, html)
                .parse_mode(ParseMode::Html)
                .await
            {
                Ok(m) => Ok(m.id),
                Err(e) => {
                    tracing::warn!("Telegram: HTML retry failed ({e}), sending as plain text");
                    let plain = strip_html_tags(html);
                    message_in_thread(bot, chat_id, thread_id, plain)
                        .await
                        .map(|m| m.id)
                }
            }
        }
        Err(e) => {
            tracing::warn!("Telegram: HTML send failed ({e}), retrying as plain text");
            let plain = strip_html_tags(html);
            message_in_thread(bot, chat_id, thread_id, plain)
                .await
                .map(|m| m.id)
        }
    }
}
