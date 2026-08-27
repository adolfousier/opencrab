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
use crate::brain::agent::service::restart_recovery;
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

/// Park a wake whose surface SDK never became ready, instead of dropping it.
///
/// The parked message is delivered when the owning channel claims the session
/// (route registration drains the parked queue), so a completion produced
/// around a restart arrives late rather than never (#1242). Callers park only
/// messages whose turn has NOT run yet — nothing re-executes on claim.
pub(crate) fn park_undeliverable(
    session_id: Uuid,
    msg: crate::brain::agent::service::QueuedUserMessage,
    surface: &str,
) {
    tracing::warn!(
        target: "bg-resume",
        "[bg-resume] {surface}: sdk unavailable after {READY_WAIT_SECS}s for \
         session {session_id} — parking until its route claim (#1242)"
    );
    restart_recovery::deliver_or_park(session_id, msg);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// First n polls miss, then the handle appears (late-connect race shape).
    #[tokio::test(start_paused = true)]
    async fn wait_ready_delivers_after_late_connect() {
        let polls = Arc::new(AtomicUsize::new(0));
        let p = polls.clone();
        let got = wait_ready(
            move || {
                let n = p.fetch_add(1, Ordering::Relaxed);
                async move { if n >= 3 { Some(7u8) } else { None } }
            },
            "test: late connect",
        )
        .await;
        assert_eq!(got, Some(7));
        assert_eq!(polls.load(Ordering::Relaxed), 4);
    }

    /// Never ready within the bound → None, bounded poll count.
    #[tokio::test(start_paused = true)]
    async fn wait_ready_times_out_bounded() {
        let polls = Arc::new(AtomicUsize::new(0));
        let p = polls.clone();
        let got = wait_ready(
            move || {
                p.fetch_add(1, Ordering::Relaxed);
                async { None::<u8> }
            },
            "test: timeout",
        )
        .await;
        assert_eq!(got, None);
        assert_eq!(polls.load(Ordering::Relaxed) as usize, READY_WAIT_SECS as usize);
    }

    /// Already-ready handle → immediate delivery, single poll, no sleep.
    #[tokio::test(start_paused = true)]
    async fn wait_ready_ready_now() {
        let polls = Arc::new(AtomicUsize::new(0));
        let p = polls.clone();
        let got = wait_ready(
            move || {
                p.fetch_add(1, Ordering::Relaxed);
                async { Some("up".to_string()) }
            },
            "test: ready now",
        )
        .await;
        assert_eq!(got.as_deref(), Some("up"));
        assert_eq!(polls.load(Ordering::Relaxed), 1);
    }

    /// Parking routes through the shared parked queue and a route claim
    /// delivers it — no loss end-to-end (#1242 contract).
    #[test]
    fn park_undeliverable_reaches_claim() {
        let _guard =
            restart_recovery::test_guard();
        let sid = Uuid::new_v4();
        let msg = crate::brain::agent::QueuedUserMessage {
            context_text: "ctx".to_string(),
            display_text: "#1242 park test".to_string(),
            origin: crate::brain::agent::PushOrigin::BackgroundTask,
        };
        park_undeliverable(sid, msg.clone(), "telegram");
        assert_eq!(
            restart_recovery::parked_count(),
            1
        );
        // A claim (what #1224 route restore does per binding) drains it.
        let seen: Arc<std::sync::Mutex<Vec<String>>> = Default::default();
        let sink = seen.clone();
        let cb: crate::brain::agent::service::MessageEnqueueCallback = Arc::new(
            move |_id, m| {
                if let Ok(mut v) = sink.lock() {
                    v.push(m.display_text);
                }
            },
        );
        let delivered =
            restart_recovery::claim_session(
                sid, &cb,
            );
        assert_eq!(delivered, 1);
        assert_eq!(seen.lock().unwrap().len(), 1);
    }
}
