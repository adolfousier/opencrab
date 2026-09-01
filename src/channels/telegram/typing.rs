//! Typing indicator that outlives the turn while work is still detached
//! (#812).
//!
//! Telegram's typing action expires after a few seconds, so it has to be
//! re-sent on a tick. The existing loops stop the moment the turn ends, which
//! is exactly wrong for a long background command: spawning one ENDS the turn,
//! so the indicator dies at the instant the user most needs it, and the chat
//! looks dead for however long the command runs. Nothing else on a channel says
//! work is in progress; the TUI has its border indicator, Telegram had nothing.
//!
//! The tick keeps running while the session has detached work, which also
//! matches what happens next: the manager wakes THAT session when the command
//! finishes, so the typing indicator is telling the truth the whole time.

use std::sync::Arc;
use std::time::Duration;

use teloxide::Bot;
use teloxide::types::{ChatAction, ChatId, ThreadId};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::brain::agent::service::background_tasks::BackgroundTaskManager;

use super::send::fire_chat_action;

/// Re-send interval. Telegram clears the action after roughly five seconds, so
/// this stays under that to avoid a visible flicker between ticks.
const TICK: Duration = Duration::from_secs(4);

/// Turn-phase loop: immediate first action, then re-send every `TICK` until
/// `cancel` fires. The building block — [`spawn_typing`] layers the detached
/// tail on top of it. Surfaces that must start typing BEFORE they know the
/// session id (message intake, before session resolution) spawn this directly
/// and pair it with [`spawn_typing_after_turn`] once the id is known.
pub(crate) fn spawn_typing_turn(
    bot: Bot,
    chat_id: ChatId,
    thread_id: Option<ThreadId>,
    cancel: CancellationToken,
) {
    tokio::spawn(async move {
        // Fire immediately: the user JUST sent a message — show typing now,
        // not after the first tick.
        fire_chat_action(&bot, chat_id, thread_id, ChatAction::Typing, "typing loop").await;
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = tokio::time::sleep(TICK) => {
                    fire_chat_action(&bot, chat_id, thread_id, ChatAction::Typing, "typing tick")
                        .await;
                }
            }
        }
    });
}

/// Keep "typing" alive for the turn, then for as long as the session has
/// background commands running.
///
/// `cancel` ends the turn phase, exactly as before. `background` is `None` on
/// surfaces with no background manager wired, which collapses this to the
/// turn loop only.
pub(crate) fn spawn_typing(
    bot: Bot,
    chat_id: ChatId,
    thread_id: Option<ThreadId>,
    cancel: CancellationToken,
    background: Option<Arc<BackgroundTaskManager>>,
    session_id: Uuid,
) {
    spawn_typing_turn(bot.clone(), chat_id, thread_id, cancel.clone());
    spawn_typing_after_turn(bot, chat_id, thread_id, cancel, background, session_id);
}

/// The tail of [`spawn_typing`], for callers that only learn their session id
/// after the turn's own typing loop has already started.
///
/// Awaits `cancel`, then keeps the indicator alive while detached work runs.
/// Pair it with an existing turn loop; it sends nothing until that loop ends.
pub(crate) fn spawn_typing_after_turn(
    bot: Bot,
    chat_id: ChatId,
    thread_id: Option<ThreadId>,
    cancel: CancellationToken,
    background: Option<Arc<BackgroundTaskManager>>,
    session_id: Uuid,
) {
    tokio::spawn(async move {
        cancel.cancelled().await;
        keep_typing_while_detached(bot, chat_id, thread_id, background, session_id).await;
    });
}

/// Tick "typing" for as long as `session_id` has background commands running.
async fn keep_typing_while_detached(
    bot: Bot,
    chat_id: ChatId,
    thread_id: Option<ThreadId>,
    background: Option<Arc<BackgroundTaskManager>>,
    session_id: Uuid,
) {
    crate::channels::typing_tick::tick_while_detached(background, session_id, TICK, || {
        fire_chat_action(
            &bot,
            chat_id,
            thread_id,
            ChatAction::Typing,
            "handover typing",
        )
    })
    .await;
}
