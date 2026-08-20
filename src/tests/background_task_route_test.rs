//! Regression (#940): a background task reports back to the surface that OWNS
//! the session, not to whichever one happened to run the command.
//!
//! Every surface builds its own `BackgroundTaskManager` from its own enqueue
//! callback, so the completion followed the executing service. A channel-bound
//! session opened in the TUI runs on the TUI's service, so the completion was
//! delivered into the TUI and the channel that asked for the work was left on
//! the agent's last "waiting for the result" message with nothing following it.

use crate::brain::agent::service::session_routes::{register_session_route, resolve_route};
use crate::brain::agent::service::{MessageEnqueueCallback, QueuedUserMessage};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

type DeliveryLog = Arc<Mutex<Vec<(&'static str, Uuid)>>>;

/// An enqueue callback that records which surface it belongs to.
fn recording_callback(name: &'static str, log: DeliveryLog) -> MessageEnqueueCallback {
    Arc::new(move |session_id, _msg: QueuedUserMessage| {
        log.lock()
            .expect("delivery log lock")
            .push((name, session_id));
    })
}

fn msg() -> QueuedUserMessage {
    QueuedUserMessage::system("context".to_string(), "display".to_string())
}

#[test]
fn a_claimed_session_resumes_on_its_own_surface_not_the_executing_one() {
    let log: DeliveryLog = Arc::new(Mutex::new(Vec::new()));
    let session = Uuid::new_v4();

    // The TUI executes the command, exactly as it does when a channel session
    // is the one open in the TUI.
    let executing = recording_callback("tui", log.clone());
    // The channel that owns the session claims its completions.
    register_session_route(session, recording_callback("channel", log.clone()));

    resolve_route(session, &executing)(session, msg());

    assert_eq!(
        *log.lock().expect("delivery log lock"),
        vec![("channel", session)],
        "the owning surface must receive the completion, and the executing one must not"
    );
}

#[test]
fn an_unclaimed_session_still_resumes_on_the_executing_surface() {
    // A genuinely TUI-local or CLI-local session has no channel to claim it.
    // The fallback must keep working or backgrounding breaks outside channels.
    let log: DeliveryLog = Arc::new(Mutex::new(Vec::new()));
    let session = Uuid::new_v4();

    let executing = recording_callback("tui", log.clone());
    resolve_route(session, &executing)(session, msg());

    assert_eq!(
        *log.lock().expect("delivery log lock"),
        vec![("tui", session)],
        "with nothing claiming the session the executing surface still delivers"
    );
}

#[test]
fn claiming_a_session_twice_keeps_the_latest_surface() {
    // Re-registration happens on every inbound message, and after a reconnect
    // the newer callback is the live one.
    let log: DeliveryLog = Arc::new(Mutex::new(Vec::new()));
    let session = Uuid::new_v4();

    let executing = recording_callback("tui", log.clone());
    register_session_route(session, recording_callback("stale", log.clone()));
    register_session_route(session, recording_callback("channel", log.clone()));

    resolve_route(session, &executing)(session, msg());

    assert_eq!(
        *log.lock().expect("delivery log lock"),
        vec![("channel", session)],
        "the most recent claim wins; a stale callback must not receive it"
    );
}

#[test]
fn one_sessions_claim_does_not_capture_another_session() {
    // The route is per session, so a claimed channel session and an unclaimed
    // local session running side by side each go to the right place. A global
    // "is a channel connected" flag would fail this.
    let log: DeliveryLog = Arc::new(Mutex::new(Vec::new()));
    let claimed = Uuid::new_v4();
    let local = Uuid::new_v4();

    let executing = recording_callback("tui", log.clone());
    register_session_route(claimed, recording_callback("channel", log.clone()));

    resolve_route(claimed, &executing)(claimed, msg());
    resolve_route(local, &executing)(local, msg());

    assert_eq!(
        *log.lock().expect("delivery log lock"),
        vec![("channel", claimed), ("tui", local)],
        "routing must be per session, not global"
    );
}
