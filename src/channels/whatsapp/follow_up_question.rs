//! WhatsApp-side rendering for the `follow_up_question` tool.
//!
//! WhatsApp's ButtonsMessage is deprecated and silently never renders
//! on the user's device, so we fall back to a numbered text list.
//! The user replies with the option number and the message router
//! resolves the pending question.

use std::sync::Arc;

use tokio::sync::oneshot;
use wacore_binary::jid::Jid;
use whatsapp_rust::client::Client;

use crate::brain::agent::{AgentError, FollowUpQuestionInfo, QuestionCallback};

/// Build the WhatsApp `QuestionCallback`. The pending question is
/// keyed by the recipient phone; the message router in `handler.rs`
/// parses the next numeric reply from that phone and resolves it.
pub(crate) fn make_question_callback(
    client: Arc<Client>,
    chat_jid: Jid,
    phone: String,
    state: Arc<super::WhatsAppState>,
) -> QuestionCallback {
    Arc::new(move |info: FollowUpQuestionInfo| {
        let client = client.clone();
        let chat_jid = chat_jid.clone();
        let phone = phone.clone();
        let state = state.clone();
        Box::pin(async move {
            let numbered: String = info
                .options
                .iter()
                .enumerate()
                .map(|(i, opt)| format!("{}. {}", i + 1, opt))
                .collect::<Vec<_>>()
                .join("\n");
            let body = format!(
                "❓ *{}*\n\n{}\n\nReply with the number of your choice.",
                info.question, numbered
            );

            let text_msg = waproto::whatsapp::Message {
                conversation: Some(body),
                ..Default::default()
            };

            let (tx, rx) = oneshot::channel::<String>();
            state
                .register_pending_question(phone.clone(), tx, info.options.clone())
                .await;
            tracing::info!(
                "WhatsApp follow_up_question: registered for phone={} options={}",
                phone,
                info.options.len()
            );

            if let Err(e) = client.send_message(chat_jid, text_msg).await {
                return Err(AgentError::Internal(format!(
                    "WhatsApp send failed: {}",
                    e
                )));
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
