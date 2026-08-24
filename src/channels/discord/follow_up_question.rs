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
///
/// `intermediate_handles` tracks in-flight intermediate text spawns.
/// Before posting the question, the callback drains and awaits all
/// pending handles so the user sees context above the buttons
/// (issue #142).
pub(crate) fn make_question_callback(
    state: Arc<super::DiscordState>,
    intermediate_handles: Arc<std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>>,
) -> QuestionCallback {
    Arc::new(move |info: FollowUpQuestionInfo| {
        let state = state.clone();
        let intermediate_handles = intermediate_handles.clone();
        Box::pin(async move {
            let http = match state.http().await {
                Some(h) => h,
                None => {
                    return Err(AgentError::Internal("Discord bot not connected".into()));
                }
            };

            // Session→owner fallback via shared helper (#764 R5).
            let channel_id = crate::channels::question_common::resolve_channel_or_error(
                state.session_channel(info.session_id),
                state.owner_channel_id(),
            )
            .await?;

            let question_id = uuid::Uuid::new_v4().to_string();

            // #1143: fold over-long labels into the question body as a
            // numbered list and render compact numeric buttons (aligned
            // with the existing 80-char truncation point). The ORIGINAL
            // strings are what gets registered below, so a click still
            // resolves to the real option text.
            let full_options = info.options.clone();
            let info = info.compact_options(80);

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
                .register_pending_question(question_id.clone(), tx, full_options)
                .await;
            tracing::info!(
                "Discord follow_up_question: registered id={} options={}",
                question_id,
                info.options.len()
            );

            // Flush in-flight intermediate text spawns before posting
            // the question, so the user sees context above the buttons
            // instead of below (issue #142). Shared drain (#764 R3).
            crate::channels::question_common::drain_intermediate_handles(
                &intermediate_handles,
                "Discord",
            )
            .await;

            if let Err(e) = ChannelId::new(channel_id)
                .send_message(&http, CreateMessage::new().content(&text).components(rows))
                .await
            {
                return Err(AgentError::Internal(format!("Discord send failed: {}", e)));
            }

            // Shared timeout ladder (#764 R2).
            crate::channels::question_common::await_answer(rx).await
        })
    })
}
