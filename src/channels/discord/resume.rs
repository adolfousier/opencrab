//! Background-task resume producer for Discord (#731).
//!
//! Mirrors Telegram's `build_enqueue_callback`: when a detached long command
//! finishes, resume the originating session and deliver the result to its
//! Discord channel. Discord has no streaming resume pipeline, so this sends the
//! completed turn's final text (like the crash-recovery path in `cli/ui.rs`).

use super::DiscordState;
use crate::brain::agent::service::MessageEnqueueCallback;
use crate::channels::bg_resume::{self, AgentHolder};
use std::sync::Arc;

pub(crate) fn build_enqueue_callback(
    state: Arc<DiscordState>,
    agent_holder: AgentHolder,
) -> MessageEnqueueCallback {
    Arc::new(move |session_id, msg| {
        let state = state.clone();
        let agent_holder = agent_holder.clone();
        tokio::spawn(async move {
            let Some(channel_id) = state.session_channel(session_id).await else {
                tracing::warn!(
                    "[bg-resume] discord: no channel for session {session_id}; dropping"
                );
                return;
            };
            // #1242: one-shot http fetch — a completion arriving while Discord
            // was still connecting was dropped outright. Bounded wait, then
            // park so the route-restore claim delivers it once connected.
            let Some(http) = bg_resume::wait_ready(|| state.http(), "discord: http").await else {
                bg_resume::park_undeliverable(session_id, msg, "discord");
                return;
            };
            let Some(agent) = bg_resume::upgrade(&agent_holder) else {
                tracing::warn!("[bg-resume] discord: agent gone; dropping resume");
                return;
            };
            let target = channel_id.to_string();
            if let Some(content) =
                bg_resume::run_resume_turn(agent, session_id, msg.context_text, "discord", &target)
                    .await
            {
                let ch = serenity::model::id::ChannelId::new(channel_id);
                if let Err(e) = ch.say(&http, &content).await {
                    tracing::warn!("[bg-resume] discord: say failed: {e}");
                }
            }
        });
    })
}
