//! A detached result queued during final delivery must not be answered by the
//! toolless flush (#1213).
//!
//! `try_begin_turn` covers the whole delivery window, not just the tool loop,
//! so a background command finishing after the loop exits is classified
//! "in-flight turn" and queued. Nothing drains it between rounds, because
//! there are no more rounds. The only remaining consumer was the #302 Stage-2
//! leftover flush, which answers through `send_message_with_display`: one
//! provider round, no tool registry. Right-sized for an emoji acknowledgement,
//! structurally wrong for a result the agent is expected to act on — the model
//! announced follow-up work it had no tools to perform, and the session went
//! quiet until the user pinged it.

use uuid::Uuid;

use crate::brain::agent::QueuedUserMessage;
use crate::channels::telegram::TelegramState;
use crate::channels::telegram::state::QueuedOrigin;

fn msg(text: &str) -> QueuedUserMessage {
    QueuedUserMessage {
        context_text: text.to_string(),
        display_text: text.to_string(),
        origin: crate::brain::agent::PushOrigin::Other,
    }
}

#[test]
fn test_origin_is_carried_through_the_queue() {
    let state = TelegramState::new();
    let session = Uuid::new_v4();

    state.enqueue_reaction(session, msg("👀 reacted"));
    state.enqueue_detached_result(session, msg("[BACKGROUND TASK DONE] exit=0"));

    let items = state.drain_queued_items(session);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].origin, QueuedOrigin::Reaction);
    assert_eq!(
        items[1].origin,
        QueuedOrigin::DetachedWork,
        "#1213: the flush cannot pick a destination it cannot tell apart"
    );
}

#[test]
fn test_the_flush_split_sends_only_detached_work_to_a_tool_loop() {
    // The partition the end-of-turn flush performs. Reactions keep the cheap
    // toolless round; detached results must not take it.
    let state = TelegramState::new();
    let session = Uuid::new_v4();

    state.enqueue_reaction(session, msg("a"));
    state.enqueue_detached_result(session, msg("b"));
    state.enqueue_reaction(session, msg("c"));

    let (detached, reactions): (Vec<_>, Vec<_>) = state
        .drain_queued_items(session)
        .into_iter()
        .partition(|i| i.origin == QueuedOrigin::DetachedWork);

    assert_eq!(detached.len(), 1);
    assert_eq!(detached[0].msg.context_text, "b");
    assert_eq!(
        reactions.len(),
        2,
        "reactions keep the flush they were built for"
    );
}

#[test]
fn test_the_live_loop_drain_is_origin_blind() {
    // Between rounds there IS a tool loop, so both kinds are injected the same
    // way. Only the end-of-turn flush needs to distinguish them, and changing
    // that would have been a behaviour change nobody asked for.
    let state = TelegramState::new();
    let session = Uuid::new_v4();

    state.enqueue_detached_result(session, msg("first"));
    state.enqueue_reaction(session, msg("second"));

    assert_eq!(state.drain_reaction(session).unwrap().context_text, "first");
    assert_eq!(
        state.drain_reaction(session).unwrap().context_text,
        "second"
    );
    assert!(state.drain_reaction(session).is_none());
}

#[test]
fn test_draining_is_per_session_and_empties_cleanly() {
    let state = TelegramState::new();
    let mine = Uuid::new_v4();
    let theirs = Uuid::new_v4();

    state.enqueue_detached_result(mine, msg("mine"));
    state.enqueue_detached_result(theirs, msg("theirs"));

    let items = state.drain_queued_items(mine);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].msg.context_text, "mine");
    assert!(
        state.drain_queued_items(mine).is_empty(),
        "a drained session must not linger in the map"
    );
    assert_eq!(state.drain_queued_items(theirs).len(), 1);
}
