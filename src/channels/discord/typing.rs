//! Sustained typing indicator for Discord (#812).
//!
//! Discord had no turn-long pinger at all. `broadcast_typing` was sent once
//! when a voice note arrived, and in a bounded 90-second burst during
//! compaction, with the code noting Discord "has no continuous typing pinger
//! like Telegram". So an ordinary turn showed the dots for a few seconds and
//! then nothing, and a detached background command showed nothing whatever,
//! since spawning one ENDS the turn.
//!
//! Discord clears the indicator after about ten seconds, so it has to be
//! re-sent on a tick like every other surface.

use std::sync::Arc;
use std::time::Duration;

use serenity::all::{ChannelId, Http};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::brain::agent::service::background_tasks::BackgroundTaskManager;

/// Re-send interval, under Discord's ~10s expiry so the dots do not flicker
/// between ticks. Matches the cadence the compaction burst already uses.
const TICK: Duration = Duration::from_secs(8);

/// Stops the indicator on drop, so every return path is covered including the
/// error ones. Matches the Telegram guard rather than relying on each early
/// return remembering to cancel.
pub(crate) struct TypingGuard(pub(crate) CancellationToken);

impl Drop for TypingGuard {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// Show "typing" for the turn, then for as long as the session has background
/// commands running.
///
/// `cancel` must be fired when the turn finishes, including on the error path,
/// or the indicator outlives the work. `background` is `None` when no manager
/// is wired, which reduces this to a plain turn-long pinger.
pub(crate) fn spawn_typing(
    http: Arc<Http>,
    channel: ChannelId,
    cancel: CancellationToken,
    background: Option<Arc<BackgroundTaskManager>>,
    session_id: Uuid,
) {
    tokio::spawn(async move {
        // Sent immediately: the point is that the user sees activity from the
        // first moment, not one tick later.
        broadcast(&http, channel, "turn start").await;
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = tokio::time::sleep(TICK) => broadcast(&http, channel, "turn tick").await,
            }
        }

        // The turn ended; keep going while detached work continues.
        crate::channels::typing_tick::tick_while_detached(background, session_id, TICK, || {
            broadcast(&http, channel, "handover")
        })
        .await;
    });
}

/// Send one typing broadcast, saying so when it fails.
///
/// The three call sites above each dropped the Result. A channel that silently
/// stops showing activity looks identical to an agent that has stopped working,
/// and there was nothing in the log to tell the two apart.
async fn broadcast(http: &Arc<Http>, channel: ChannelId, why: &str) {
    if let Err(e) = channel.broadcast_typing(http).await {
        tracing::warn!("Discord: typing broadcast failed ({why}): {e}");
    }
}
