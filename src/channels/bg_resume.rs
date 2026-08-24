//! Shared helpers for background-task resume across channels (#731).
//!
//! When a detached long command finishes, the [`BackgroundTaskManager`] calls
//! the surface's `MessageEnqueueCallback` with the originating `session_id` and
//! a synthetic completion message. Each channel's callback resolves its delivery
//! target, runs a fresh turn feeding the completion text as the prompt, and
//! delivers the result back to that target. The turn-running and weak-agent
//! plumbing is identical across channels and lives here; only target lookup and
//! the SDK-specific send call are per-channel.
//!
//! [`BackgroundTaskManager`]: crate::brain::agent::service::BackgroundTaskManager

use crate::brain::agent::AgentService;
use std::sync::{Arc, Mutex, Weak};
use uuid::Uuid;

/// Weak handle to the agent, filled after the service is built (it cannot be
/// captured at service-construction time — the service is mid-build). Every
/// channel's enqueue callback closes over one of these.
pub(crate) type AgentHolder = Arc<Mutex<Option<Weak<AgentService>>>>;

/// A fresh, empty holder to hand to `build_enqueue_callback` before the agent
/// exists; fill it with [`fill`] once the service is constructed.
pub(crate) fn new_holder() -> AgentHolder {
    Arc::new(Mutex::new(None))
}

/// Store a weak ref to the just-built agent so the enqueue callback can reach it.
pub(crate) fn fill(holder: &AgentHolder, agent: &Arc<AgentService>) {
    if let Ok(mut h) = holder.lock() {
        *h = Some(Arc::downgrade(agent));
    }
}

/// Upgrade the weak holder to a live agent, or `None` if the service is gone.
pub(crate) fn upgrade(holder: &AgentHolder) -> Option<Arc<AgentService>> {
    holder
        .lock()
        .ok()
        .and_then(|g| g.as_ref().and_then(Weak::upgrade))
}

/// Run the background-completion turn for `session_id`, feeding `context_text`
/// as the prompt on `channel`/`target`. Returns the response content, or `None`
/// when the turn errored or produced nothing to deliver. Delivery to the
/// channel's SDK is the caller's job (it differs per surface).
pub(crate) async fn run_resume_turn(
    agent: Arc<AgentService>,
    session_id: Uuid,
    context_text: String,
    channel: &str,
    target: &str,
) -> Option<String> {
    match agent
        .send_message_with_tools_and_callback(
            session_id,
            context_text,
            None,
            None,
            None,
            None,
            channel,
            Some(target),
        )
        .await
    {
        Ok(resp) if !resp.content.trim().is_empty() => Some(resp.content),
        Ok(_) => None,
        Err(e) => {
            tracing::warn!(
                "[bg-resume] {channel}: resume turn failed for session {session_id}: {e}"
            );
            None
        }
    }
}
