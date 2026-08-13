//! Restart reports reach the session that owns the work (#1037).
//!
//! Recovery runs during startup, before any channel registers a route, so a
//! report produced then has nobody to deliver to yet. It must park rather than
//! fall back to whoever booted, leave the moment a route appears, and still be
//! delivered locally if no route ever does.

use std::sync::Arc;
use std::sync::Mutex;

use uuid::Uuid;

use crate::brain::agent::QueuedUserMessage;
use crate::brain::agent::service::MessageEnqueueCallback;
use crate::brain::agent::service::background_tasks::register_session_route;
use crate::brain::agent::service::restart_recovery::{
    clear_parked_for_test, deliver_or_park, flush_parked, parked_count,
};

/// What a recording callback saw: (session, display text) per delivery.
type Seen = Arc<Mutex<Vec<(Uuid, String)>>>;

/// The parked queue is process-global, so these tests cannot run alongside
/// each other: one test's park inflates another's count. Serialize them.
static TEST_LOCK: Mutex<()> = Mutex::new(());

/// Take the serialization lock, recovering from a poisoned mutex so one
/// failing test does not cascade into the rest.
fn serialized() -> std::sync::MutexGuard<'static, ()> {
    let guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_parked_for_test();
    guard
}

/// A callback that records what it was handed.
fn recorder() -> (MessageEnqueueCallback, Seen) {
    let seen: Arc<Mutex<Vec<(Uuid, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    let cb: MessageEnqueueCallback = Arc::new(move |id: Uuid, msg: QueuedUserMessage| {
        if let Ok(mut v) = sink.lock() {
            v.push((id, msg.display_text));
        }
    });
    (cb, seen)
}

fn msg(text: &str) -> QueuedUserMessage {
    QueuedUserMessage {
        context_text: text.to_string(),
        display_text: text.to_string(),
    }
}

#[test]
fn a_report_with_no_route_parks_instead_of_going_out() {
    let _guard = serialized();
    let session = Uuid::new_v4();

    let delivered = deliver_or_park(session, msg("interrupted"));

    assert!(!delivered, "nothing claims this session yet");
    assert_eq!(parked_count(), 1);
}

#[test]
fn registering_a_route_hands_over_what_was_parked() {
    let _guard = serialized();
    let session = Uuid::new_v4();
    deliver_or_park(session, msg("interrupted"));

    let (cb, seen) = recorder();
    register_session_route(session, cb);

    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1, "the channel gets what it missed");
    assert_eq!(seen[0].0, session);
    assert_eq!(seen[0].1, "interrupted");
    assert_eq!(parked_count(), 0);
}

#[test]
fn a_route_that_already_exists_receives_immediately() {
    let _guard = serialized();
    let session = Uuid::new_v4();
    let (cb, seen) = recorder();
    register_session_route(session, cb);

    let delivered = deliver_or_park(session, msg("later report"));

    assert!(delivered);
    assert_eq!(parked_count(), 0, "nothing needed parking");
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[test]
fn one_sessions_route_does_not_drain_anothers_reports() {
    // The whole point is per-session delivery: claiming one session must not
    // hand it work belonging to a different one.
    let _guard = serialized();
    let mine = Uuid::new_v4();
    let theirs = Uuid::new_v4();
    deliver_or_park(mine, msg("mine"));
    deliver_or_park(theirs, msg("theirs"));

    let (cb, seen) = recorder();
    register_session_route(mine, cb);

    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].1, "mine");
    assert_eq!(parked_count(), 1, "the other session's report still waits");
}

#[test]
fn the_flush_delivers_locally_rather_than_losing_the_report() {
    // A channel that never comes up in this run must not strand the report
    // forever; the local surface is the last resort, not the first choice.
    let _guard = serialized();
    let orphan = Uuid::new_v4();
    deliver_or_park(orphan, msg("nobody claimed me"));

    let (local, seen) = recorder();
    let flushed = flush_parked(&local);

    assert_eq!(flushed, 1);
    assert_eq!(parked_count(), 0);
    let seen = seen.lock().unwrap();
    assert_eq!(seen[0].0, orphan);
    assert_eq!(seen[0].1, "nobody claimed me");
}

#[test]
fn flushing_with_nothing_parked_is_a_no_op() {
    let _guard = serialized();
    let (local, seen) = recorder();

    assert_eq!(flush_parked(&local), 0);
    assert!(seen.lock().unwrap().is_empty());
}
