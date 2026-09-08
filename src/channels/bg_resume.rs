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
///
/// Tracked with origin `system` (#12): a completion turn killed mid-tool must
/// be visible to boot recovery, which re-delivers the push instead of
/// replaying the LLM turn.
pub(crate) async fn run_resume_turn(
    agent: Arc<AgentService>,
    session_id: Uuid,
    context_text: String,
    channel: &str,
    target: &str,
) -> Option<String> {
    match agent
        .send_push_turn(
            session_id,
            context_text,
            None,
            None,
            None,
            None,
            channel,
            Some(target),
            None,
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

/// How long a channel SDK handle may take to become ready after spawn before
/// a pending wake stops waiting and parks instead (#1242).
///
/// Covers the boot race observed in the 2026-08-26/27 logs: an enqueue
/// callback fired while the bot was still authenticating and the wake was
/// dropped outright. The bound matches the pre-existing inline waits
/// (telegram `wait_for_bot`, the ui.rs startup-resume loop) so all callers
/// share one number.
pub(crate) const READY_WAIT_SECS: u64 = 30;

/// Poll `fetch` once a second until it yields a value or [`READY_WAIT_SECS`]
/// elapses.
///
/// Replaces the one-shot handle fetches that dropped the wake whenever the
/// SDK client was not ready at the exact instant of the call (#1242).
/// Checks before sleeping, so an already-ready handle costs zero delay.
pub(crate) async fn wait_ready<T, F, Fut>(mut fetch: F, label: &str) -> Option<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    for _ in 0..READY_WAIT_SECS {
        if let Some(value) = fetch().await {
            return Some(value);
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    tracing::warn!(target: "bg-resume", "{label}: still unavailable after {READY_WAIT_SECS}s");
    None
}
