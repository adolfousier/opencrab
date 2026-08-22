//! Telegram-side rendering for the `follow_up_question` tool.
//!
//! Builds a `QuestionCallback` that sends an inline-keyboard message
//! with one button per option, suspends on a oneshot until the user
//! taps, and returns the chosen option string to the tool.
//!
//! Lives in its own module to keep the already-large `handler.rs`
//! focused on the message-routing path.

use std::sync::Arc;

use teloxide::payloads::SendMessageSetters;
use teloxide::types::{ChatId, InlineKeyboardButton, InlineKeyboardMarkup, ParseMode};
use tokio::sync::oneshot;

use super::handler::{StreamingState, flush_intermediates};
use crate::brain::agent::{AgentError, FollowUpQuestionInfo, QuestionCallback};

/// Escape the four HTML-special characters teloxide's `ParseMode::Html`
/// recognises. Mirrors the helper in `handler.rs` but is private here
/// so the two modules stay independent.
fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Build the Telegram `QuestionCallback`. Each invocation renders the
/// question + buttons, registers a pending entry on the state, and
/// blocks on the matching oneshot.
///
/// `streaming` is shared with the per-turn edit loop. Before posting
/// the question, the callback drains any pending intermediate texts
/// from the display queue and sends them synchronously so the user
/// sees context above the buttons (issue #142).
pub(crate) fn make_question_callback(
    state: Arc<super::TelegramState>,
    streaming: Arc<std::sync::Mutex<StreamingState>>,
) -> QuestionCallback {
    Arc::new(move |info: FollowUpQuestionInfo| {
        let state = state.clone();
        let streaming = streaming.clone();
        Box::pin(async move {
            let chat_id = match state.session_chat(info.session_id).await {
                Some(id) => id,
                None => match state.owner_chat_id().await {
                    Some(id) => id,
                    None => {
                        tracing::warn!(
                            "Telegram follow_up_question: no chat_id for session {}",
                            info.session_id
                        );
                        return Err(AgentError::Internal("no chat_id for session".into()));
                    }
                },
            };

            let bot = match state.bot().await {
                Some(b) => b,
                None => {
                    tracing::warn!("Telegram follow_up_question: bot not connected");
                    return Err(AgentError::Internal("bot not connected".into()));
                }
            };

            let question_id = uuid::Uuid::new_v4().to_string();

            // #1143: if any option label is over-long for one-per-row
            // inline buttons, fold the full texts into the question body
            // as a numbered list and render compact numeric buttons.
            // The ORIGINAL strings are what gets registered below, so a
            // tap still resolves to the real option text — the model
            // never sees a bare number.
            let full_options = info.options.clone();
            let info = info.compact_options(40);

            // Single-column layout. Each option gets its own row so
            // labels stay readable on narrow screens. The absolute
            // option index is encoded in the callback data so the
            // click handler can map back to the chosen option string
            // via the stored options list.
            let keyboard_rows: Vec<Vec<InlineKeyboardButton>> = info
                .options
                .iter()
                .enumerate()
                .map(|(i, opt)| {
                    vec![InlineKeyboardButton::callback(
                        opt.clone(),
                        format!("q:{}:{}", question_id, i),
                    )]
                })
                .collect();
            let keyboard = InlineKeyboardMarkup::new(keyboard_rows);

            let text = format!("❓ <b>{}</b>", escape_html(&info.question));

            let (tx, rx) = oneshot::channel::<String>();
            state
                .register_pending_question(question_id.clone(), info.session_id, tx, full_options)
                .await;
            tracing::info!(
                "Telegram follow_up_question: registered id={} options={}",
                question_id,
                info.options.len()
            );

            // Resolve thread_id for this chat (forum topic routing #247).
            // Use the in-memory session_topic map (populated when the message
            // arrived) instead of the DB query (latest_thread_id_for_chat)
            // which has a race on first-message-in-topic and can pick the
            // wrong topic when multiple topics are active.
            let thread_id = state
                .session_topic(info.session_id)
                .await
                .map(|tid| teloxide::types::ThreadId(teloxide::types::MessageId(tid)));

            // Flush any pending intermediate texts BEFORE the question
            // lands. Without this, the 1500ms edit loop sends them
            // after the buttons, confusing the user (issue #142).
            flush_intermediates(&bot, ChatId(chat_id), thread_id, &streaming).await;

            match super::send::message_in_thread(&bot, ChatId(chat_id), thread_id, &text)
                .parse_mode(ParseMode::Html)
                .reply_markup(keyboard)
                .await
            {
                Ok(sent) => {
                    // The question bubble is non-sticky burial evidence (#1150):
                    // when the turn resumes and appends more rounds, the flow
                    // block must restick BELOW this Q/A pair instead of staying
                    // pinned above it.
                    state.note_bot_bubble(chat_id, sent.id.0);
                }
                Err(e) => {
                    tracing::error!("Telegram follow_up_question: send failed: {}", e);
                    return Err(AgentError::Internal(format!("send failed: {}", e)));
                }
            }

            match tokio::time::timeout(std::time::Duration::from_secs(600), rx).await {
                Ok(Ok(answer)) => {
                    tracing::info!(
                        "Telegram follow_up_question: answered id={} choice={:?}",
                        question_id,
                        answer
                    );
                    Ok(answer)
                }
                Ok(Err(_)) => {
                    // Sender dropped without firing: clear both maps so no dead
                    // reverse entry lingers to swallow the next text (#500).
                    state
                        .clear_pending_question(&question_id, info.session_id)
                        .await;
                    Err(AgentError::Internal(
                        "follow_up_question oneshot channel closed".into(),
                    ))
                }
                Err(_) => {
                    tracing::warn!(
                        "Telegram follow_up_question: 10-minute timeout id={}",
                        question_id
                    );
                    // Same cleanup on timeout: the question is gone, drop its
                    // reverse mapping (#500).
                    state
                        .clear_pending_question(&question_id, info.session_id)
                        .await;
                    Err(AgentError::Internal("follow_up_question timed out".into()))
                }
            }
        })
    })
}
