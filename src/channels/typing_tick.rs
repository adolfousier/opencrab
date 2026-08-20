//! Keeping a channel's typing indicator alive while work is detached (#812).
//!
//! Every channel expires its indicator after a few seconds, so it has to be
//! re-sent on a tick, and every channel needs the same unusual tail: spawning
//! a detached command ENDS the turn, so the turn's own loop stops at exactly
//! the moment the user most needs to see that something is happening.
//!
//! Telegram and Discord had that tail written out twice, with the same
//! before-sleeping ordering and the same rationale in both comments. Only the
//! ping differs, so only the ping is a parameter.

use std::sync::Arc;
use std::time::Duration;

use uuid::Uuid;

use crate::brain::agent::service::background_tasks::BackgroundTaskManager;

/// Ping `send` every `tick` for as long as `session_id` has detached work.
///
/// Returns immediately when no manager is wired, which is what a surface
/// without background support needs. The first ping goes out BEFORE the first
/// sleep: the turn has just ended and its own last ping is already expiring,
/// so waiting a full tick would leave a visible dead gap at the handover.
pub(crate) async fn tick_while_detached<F, Fut>(
    background: Option<Arc<BackgroundTaskManager>>,
    session_id: Uuid,
    tick: Duration,
    mut send: F,
) where
    F: FnMut() -> Fut,
    Fut: Future<Output = ()>,
{
    let Some(manager) = background else {
        return;
    };
    while manager.running_for(session_id) > 0 {
        send().await;
        tokio::time::sleep(tick).await;
    }
}
