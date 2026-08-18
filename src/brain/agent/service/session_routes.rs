//! Where a background-task completion is delivered (#940, #1036).
//!
//! Delivery is keyed by SESSION, never by whichever surface happened to run
//! the command. Every surface builds its own manager from its own enqueue
//! callback, so routing by the executing service sent a channel-bound session
//! driven from the TUI back to the TUI, and the channel that asked for the
//! work never heard the answer.
//!
//! Split out of the task manager because none of it knows anything about
//! tasks: it is a session-to-callback registry that spawning, sub-agents and
//! restart recovery all consult. Keeping it next to spawning meant three
//! unrelated concerns shared one file and one set of locks to reason about.

use std::collections::HashMap;
use std::sync::Mutex;

use uuid::Uuid;

use super::types::{MessageEnqueueCallback, QueuedUserMessage};

/// Where a session's background-task completion must be delivered, keyed by
/// session rather than by whichever surface happened to run the command.
///
/// Every surface builds its own `BackgroundTaskManager` from its own enqueue
/// callback, so the completion used to follow the *executing* service. A
/// channel-bound session driven from the TUI therefore reported back to the
/// TUI, and the channel that started the work never heard the answer (#940).
/// A channel registers its session here when it binds one; the manager
/// consults this first and only falls back to its own callback when nothing
/// claims the session (a genuinely TUI-local or CLI-local session).
static SESSION_ROUTES: Mutex<Option<HashMap<Uuid, MessageEnqueueCallback>>> = Mutex::new(None);

/// Bind `session_id`'s background-task completions to `enqueue`.
///
/// Idempotent: re-binding the same session replaces the route, which is what
/// a reconnect or a bot restart needs.
pub fn register_session_route(session_id: Uuid, enqueue: MessageEnqueueCallback) {
    match SESSION_ROUTES.lock() {
        Ok(mut guard) => {
            guard
                .get_or_insert_with(HashMap::new)
                .insert(session_id, enqueue.clone());
            // Startup recovery runs before any channel connects, so this
            // session may already have reports waiting for someone to claim
            // it. Hand them over now that there is somewhere to send them
            // (#1037). Done after the insert so the route is live first.
            super::restart_recovery::claim_session(session_id, &enqueue);
        }
        Err(e) => {
            // Worth saying out loud: without the route this session's next
            // background completion silently goes to the wrong surface.
            tracing::error!(
                target: "background_task",
                "Could not register resume route for session {session_id}: {e}"
            );
        }
    }
}

/// Who should receive `session_id`'s completion: the surface that claimed the
/// session, falling back to `executing` when nothing did.
///
/// The whole fix in one line — pick by session, never by who ran the command —
/// so it is a pure function and directly testable.
pub fn resolve_route(
    session_id: Uuid,
    executing: &MessageEnqueueCallback,
) -> MessageEnqueueCallback {
    session_route(session_id).unwrap_or_else(|| executing.clone())
}

/// The surface this process booted on, used when no channel claims a session.
///
/// `spawn_command` carries the executing service's callback on the manager, so
/// it always has a fallback. A sub-agent has no such handle — it is reached
/// from a tool with no service context — so the local surface is registered
/// once at startup and resolved on demand instead (#1036).
static LOCAL_ROUTE: Mutex<Option<MessageEnqueueCallback>> = Mutex::new(None);

/// Record the booting surface as the fallback destination. Called once per
/// process start; re-registering replaces it.
pub fn register_local_route(enqueue: MessageEnqueueCallback) {
    match LOCAL_ROUTE.lock() {
        Ok(mut guard) => *guard = Some(enqueue),
        Err(e) => {
            // Without it, a sub-agent finishing on a session no channel owns
            // has nowhere to report and its output is dropped.
            tracing::error!(
                target: "background_task",
                "Could not register the local delivery route: {e}"
            );
        }
    }
}

/// Deliver `msg` to whoever owns `session_id`, falling back to the booting
/// surface. Returns whether it went anywhere at all.
pub fn deliver_to_session(session_id: Uuid, msg: QueuedUserMessage) -> bool {
    if let Some(route) = session_route(session_id) {
        route(session_id, msg);
        return true;
    }
    let local = match LOCAL_ROUTE.lock() {
        Ok(guard) => guard.clone(),
        Err(e) => {
            tracing::error!(
                target: "background_task",
                "Could not read the local delivery route for session {session_id}: {e}"
            );
            None
        }
    };
    match local {
        Some(route) => {
            route(session_id, msg);
            true
        }
        None => {
            tracing::error!(
                target: "background_task",
                "Nothing can receive a message for session {session_id}; it is dropped: {}",
                msg.display_text
            );
            false
        }
    }
}

/// The surface that owns `session_id`'s completions, if one claimed it.
pub fn session_route(session_id: Uuid) -> Option<MessageEnqueueCallback> {
    match SESSION_ROUTES.lock() {
        Ok(guard) => guard.as_ref()?.get(&session_id).cloned(),
        Err(e) => {
            tracing::error!(
                target: "background_task",
                "Could not read resume route for session {session_id}: {e}"
            );
            None
        }
    }
}
