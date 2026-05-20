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
pub(crate) fn make_question_callback(state: Arc<super::SlackState>) -> QuestionCallback {
    Arc::new(move |info: FollowUpQuestionInfo| {
        let state = state.clone();
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

            let header = SlackBlock::Section(
                SlackSectionBlock::new().with_text(SlackBlockText::MarkDown(
                    SlackBlockMarkDownText::new(format!("❓ *{}*", info.question)),
                )),
            );
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

            if let Err(e) = session.chat_post_message(&request).await {
                return Err(AgentError::Internal(format!("Slack send failed: {}", e)));
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
