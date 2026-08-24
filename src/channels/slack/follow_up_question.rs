//! Slack-side rendering for the `follow_up_question` tool.
//!
//! Posts a Block Kit message with one button per option (Slack
//! ActionsBlock), suspends on a oneshot until the user clicks, and
//! resolves with the chosen option string.

use std::sync::Arc;

use slack_morphism::prelude::*;
use tokio::sync::oneshot;

use crate::brain::agent::{AgentError, FollowUpQuestionInfo, QuestionCallback};

/// Build the Slack `QuestionCallback`.
///
/// `intermediate_handles` tracks in-flight intermediate text spawns.
/// Before posting the question, the callback drains and awaits all
/// pending handles so the user sees context above the buttons
/// (issue #142).
pub(crate) fn make_question_callback(
    state: Arc<super::SlackState>,
    intermediate_handles: Arc<std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>>,
) -> QuestionCallback {
    Arc::new(move |info: FollowUpQuestionInfo| {
        let state = state.clone();
        let intermediate_handles = intermediate_handles.clone();
        Box::pin(async move {
            let client = match state.client().await {
                Some(c) => c,
                None => {
                    return Err(AgentError::Internal("Slack bot not connected".into()));
                }
            };

            let bot_token = match state.bot_token().await {
                Some(t) => t,
                None => return Err(AgentError::Internal("Slack: no bot token".into())),
            };

            // Session→owner fallback via shared helper (#764 R5).
            let channel_id = crate::channels::question_common::resolve_channel_or_error(
                state.session_channel(info.session_id),
                state.owner_channel_id(),
            )
            .await?;

            let question_id = uuid::Uuid::new_v4().to_string();

            // One Slack ActionsBlock allows up to 25 elements — well
            // above our 8-option cap — so one block holds everything.
            let buttons: Vec<SlackActionBlockElement> = info
                .options
                .iter()
                .enumerate()
                .map(|(idx, opt)| {
                    SlackActionBlockElement::Button(SlackBlockButtonElement::new(
                        SlackActionId::new(format!("q:{}:{}", question_id, idx)),
                        SlackBlockPlainTextOnly::from(SlackBlockPlainText::new(opt.clone())),
                    ))
                })
                .collect();

            let header =
                SlackBlock::Section(SlackSectionBlock::new().with_text(SlackBlockText::MarkDown(
                    SlackBlockMarkDownText::new(format!("❓ *{}*", info.question)),
                )));
            let actions = SlackBlock::Actions(SlackActionsBlock::new(buttons));

            let content = SlackMessageContent::new()
                .with_text(info.question.clone())
                .with_blocks(vec![header, actions]);
            let request = SlackApiChatPostMessageRequest::new(
                SlackChannelId::new(channel_id.clone()),
                content,
            );
            let token = SlackApiToken::new(SlackApiTokenValue::from(bot_token.clone()));
            let session = client.open_session(&token);

            let (tx, rx) = oneshot::channel::<String>();
            state
                .register_pending_question(question_id.clone(), tx, info.options.clone())
                .await;
            tracing::info!(
                "Slack follow_up_question: registered id={} options={}",
                question_id,
                info.options.len()
            );

            // Flush in-flight intermediate text spawns before posting
            // the question, so the user sees context above the buttons
            // instead of below (issue #142).
            // Shared drain (#764 R3).
            crate::channels::question_common::drain_intermediate_handles(
                &intermediate_handles,
                "Slack",
            )
            .await;

            if let Err(e) = session.chat_post_message(&request).await {
                return Err(AgentError::Internal(format!("Slack send failed: {}", e)));
            }

            // Shared timeout ladder (#764 R2).
            crate::channels::question_common::await_answer(rx).await
        })
    })
}
