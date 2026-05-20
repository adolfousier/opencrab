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
use teloxide::prelude::Requester;
use teloxide::types::{ChatId, InlineKeyboardButton, InlineKeyboardMarkup, ParseMode};
use tokio::sync::oneshot;

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
pub(crate) fn make_question_callback(state: Arc<super::TelegramState>) -> QuestionCallback {
    Arc::new(move |info: FollowUpQuestionInfo| {
        let state = state.clone();
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

            // Two-column layout. The absolute option index is encoded
            // in the callback data so the click handler can map back
            // to the chosen option string via the stored options list.
            let keyboard_rows: Vec<Vec<InlineKeyboardButton>> = info
                .options
                .iter()
                .enumerate()
                .fold(Vec::new(), |mut acc, (i, opt)| {
                    let btn = InlineKeyboardButton::callback(
                        opt.clone(),
                        format!("q:{}:{}", question_id, i),
                    );
                    if i % 2 == 0 {
                        acc.push(vec![btn]);
                    } else if let Some(last) = acc.last_mut() {
                        last.push(btn);
                    } else {
                        acc.push(vec![btn]);
                    }
                    acc
                });
            let keyboard = InlineKeyboardMarkup::new(keyboard_rows);

            let text = format!("❓ <b>{}</b>", escape_html(&info.question));

            let (tx, rx) = oneshot::channel::<String>();
            state
                .register_pending_question(question_id.clone(), tx, info.options.clone())
                .await;
            tracing::info!(
                "Telegram follow_up_question: registered id={} options={}",
                question_id,
                info.options.len()
            );

            if let Err(e) = bot
                .send_message(ChatId(chat_id), &text)
                .parse_mode(ParseMode::Html)
                .reply_markup(keyboard)
                .await
            {
                tracing::error!("Telegram follow_up_question: send failed: {}", e);
                return Err(AgentError::Internal(format!("send failed: {}", e)));
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
                Ok(Err(_)) => Err(AgentError::Internal(
                    "follow_up_question oneshot channel closed".into(),
                )),
                Err(_) => {
                    tracing::warn!(
                        "Telegram follow_up_question: 10-minute timeout id={}",
                        question_id
                    );
                    Err(AgentError::Internal("follow_up_question timed out".into()))
                }
            }
        })
    })
}
