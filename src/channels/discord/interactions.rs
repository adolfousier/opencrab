//! Agent-driven interactive components: select menus (#382) and modal
//! forms (#383), with lazy TTL expiry (#386).
//!
//! `discord_send` posts a select menu or a form button; the pick or the
//! submitted fields come back here, get routed into the channel's session
//! as an agent turn (with only a compact "[System: ...]" tag persisted),
//! and the reply is delivered to the channel. Pending component state
//! lives in [`super::DiscordState`] with creation timestamps; clicks past
//! the TTL answer "expired" instead of firing stale actions.

use crate::brain::agent::AgentService;
use crate::services::SessionService;
use serenity::prelude::Context;
use std::sync::Arc;
use uuid::Uuid;

/// One pending modal form: what the modal shows when the button is hit.
#[derive(Debug, Clone)]
pub(crate) struct FormSpec {
    pub title: String,
    /// (label, multiline) per field, max 5 (Discord's modal cap).
    pub fields: Vec<(String, bool)>,
}

/// Resolve (or create) the session for interaction input, mirroring
/// handle_message's keying: DMs by user, channels/threads by channel id.
pub(crate) async fn resolve_interaction_session(
    session_svc: &SessionService,
    is_dm: bool,
    user_id: u64,
    channel_id: u64,
    idle_hours: Option<f64>,
) -> Option<Uuid> {
    use crate::channels::session_resolve;
    let (id_str, legacy_title) = if is_dm {
        (
            format!("discord-dm-{user_id}"),
            format!("Discord: DM {user_id}"),
        )
    } else {
        (
            format!("discord-{channel_id}"),
            format!("Discord: #{channel_id}"),
        )
    };
    let suffix = session_resolve::chat_id_suffix(&id_str);
    let session_title = format!("{legacy_title} {suffix}");
    match session_resolve::resolve_or_create_channel_session(
        session_svc,
        &suffix,
        &legacy_title,
        &session_title,
        idle_hours,
        "Discord",
    )
    .await
    {
        Ok(id) => Some(id),
        Err(e) => {
            tracing::error!("Discord interaction: failed to resolve session: {e}");
            None
        }
    }
}

/// Run an interaction-originated agent turn and deliver the reply to the
/// channel. `context_text` goes to the model for this turn only; history
/// persists `display_tag` (the system-note contract shared with reactions).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn route_interaction_turn(
    ctx: &Context,
    agent: Arc<AgentService>,
    session_svc: SessionService,
    is_dm: bool,
    user_id: u64,
    channel_id: u64,
    idle_hours: Option<f64>,
    context_text: String,
    display_tag: String,
) {
    let Some(session_id) =
        resolve_interaction_session(&session_svc, is_dm, user_id, channel_id, idle_hours).await
    else {
        return;
    };
    let response = match agent
        .send_message_with_display(session_id, context_text, Some(display_tag), None)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("Discord interaction: agent error for session {session_id}: {e}");
            return;
        }
    };
    let (text_only, _imgs) = crate::utils::extract_img_markers(&response.content);
    let text_only = crate::utils::sanitize::strip_llm_artifacts(&text_only);
    let (text_only, _react) = crate::utils::extract_react_marker(&text_only);
    let trimmed = text_only.trim();
    if trimmed.is_empty() {
        return;
    }
    let channel = serenity::model::id::ChannelId::new(channel_id);
    for chunk in super::handler::split_message(trimmed, 2000) {
        if let Err(e) = channel.say(&ctx.http, chunk).await {
            tracing::warn!("Discord interaction: failed to deliver reply: {e}");
        }
    }
}
