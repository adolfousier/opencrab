//! Background-task resume producer for WhatsApp (#731).
//!
//! Mirrors Telegram's `build_enqueue_callback`: when a detached long command
//! finishes, resume the originating session and send the result to its chat.
//! The session→JID map is populated per turn in the handler (`register_session_jid`).

use super::WhatsAppState;
use crate::brain::agent::service::MessageEnqueueCallback;
use crate::channels::bg_resume::{self, AgentHolder};
use std::sync::Arc;

pub(crate) fn build_enqueue_callback(
    state: Arc<WhatsAppState>,
    agent_holder: AgentHolder,
) -> MessageEnqueueCallback {
    Arc::new(move |session_id, msg| {
        let state = state.clone();
        let agent_holder = agent_holder.clone();
        tokio::spawn(async move {
            let Some(jid_str) = state.session_jid(session_id).await else {
                tracing::warn!(
                    "[bg-resume] whatsapp: no chat jid for session {session_id}; dropping"
                );
                return;
            };
            let Some(agent) = bg_resume::upgrade(&agent_holder) else {
                tracing::warn!("[bg-resume] whatsapp: agent gone; dropping resume");
                return;
            };
            let Some(content) = bg_resume::run_resume_turn(
                agent,
                session_id,
                msg.context_text,
                "whatsapp",
                &jid_str,
            )
            .await
            else {
                return;
            };
            let Some(client) = state.client().await else {
                tracing::warn!("[bg-resume] whatsapp: client not available; dropping delivery");
                return;
            };
            let Ok(jid) = jid_str.parse::<wacore_binary::jid::Jid>() else {
                tracing::warn!("[bg-resume] whatsapp: bad jid '{jid_str}'; dropping delivery");
                return;
            };
            let out = waproto::whatsapp::Message {
                conversation: Some(content),
                ..Default::default()
            };
            if let Err(e) = client.send_message(jid, out).await {
                tracing::warn!("[bg-resume] whatsapp: send_message failed: {e}");
            }
        });
    })
}
