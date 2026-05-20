//! Discord-side rendering for the `follow_up_question` tool.
//!
//! Builds a `QuestionCallback` that posts an interactive message with
//! one Secondary-style button per option (up to 5 per ActionRow),
//! suspends on a oneshot until the user clicks, and resolves with the
//! chosen option string.
//!
//! Extracted from `handler.rs` to keep the message-routing path lean.

use std::sync::Arc;

use serenity::builder::{CreateActionRow, CreateButton, CreateMessage};
use serenity::model::application::ButtonStyle;
use serenity::model::id::ChannelId;
use tokio::sync::oneshot;

use crate::brain::agent::{AgentError, FollowUpQuestionInfo, QuestionCallback};
use crate::utils::truncate_str;

/// Build the Discord `QuestionCallback`.
pub(crate) fn make_question_callback(state: Arc<super::DiscordState>) -> QuestionCallback {
    Arc::new(move |info: FollowUpQuestionInfo| {
        let state = state.clone();
        Box::pin(async move {
            let http = match state.http().await {
                Some(h) => h,
                None => {
                    return Err(AgentError::Internal("Discord bot not connected".into()));
                }
            };

            let channel_id = match state.session_channel(info.session_id).await {
                Some(id) => id,
                None => match state.owner_channel_id().await {
                    Some(id) => id,
                    None => {
                        return Err(AgentError::Internal("no channel_id for session".into()));
                    }
                },
            };

            let question_id = uuid::Uuid::new_v4().to_string();

            // Discord ActionRows allow up to 5 buttons. follow_up_
            // question caps at 8 options so we split into at most 2
            // rows. The absolute option index is encoded in the
            // custom_id so the interaction handler can map back to the
            // chosen option string via the stored options list.
            let rows: Vec<CreateActionRow> = info
                .options
                .iter()
                .enumerate()
                .collect::<Vec<_>>()
                .chunks(5)
                .map(|chunk| {
                    CreateActionRow::Buttons(
                        chunk
                            .iter()
                            .map(|(idx, opt)| {
                                CreateButton::new(format!("q:{}:{}", question_id, idx))
                                    .label(truncate_str(opt, 80))
                                    .style(ButtonStyle::Secondary)
                            })
                            .collect(),
                    )
                })
                .collect();

            let text = format!("❓ **{}**", info.question);

            let (tx, rx) = oneshot::channel::<String>();
            state
                .register_pending_question(question_id.clone(), tx, info.options.clone())
                .await;
            tracing::info!(
                "Discord follow_up_question: registered id={} options={}",
                question_id,
                info.options.len()
            );

            if let Err(e) = ChannelId::new(channel_id)
                .send_message(&http, CreateMessage::new().content(&text).components(rows))
                .await
            {
                return Err(AgentError::Internal(format!("Discord send failed: {}", e)));
            }

            match tokio::time::timeout(std::time::Duration::from_secs(600), rx).await {
                Ok(Ok(answer)) => Ok(answer),
                Ok(Err(_)) => Err(AgentError::Internal(
                    "follow_up_question oneshot closed".into(),
                )),
                Err(_) => Err(AgentError::Internal("follow_up_question timed out".into())),
            }
        })
    })
}
