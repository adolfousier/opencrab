//! A session revived by startup crash-recovery must not be treated as local
//! (#1206).
//!
//! Recovery replays an interrupted turn directly, bypassing the ingress
//! handlers where `claim_for_channel` lives, so the route map says nothing
//! about the session while that turn runs. Both fallbacks then read "no
//! route" as "local session":
//!
//! * on a headless daemon the local surface is the TUI event channel, which
//!   nothing drains, so the completion vanished with no log at all;
//! * under the TUI it is worse than a void, because the answer arrives on
//!   the wrong surface and looks delivered.
//!
//! Recovery knows which channel the session came from, so it marks it and
//! both fallbacks park until that channel claims it.

use std::sync::Arc;
use std::sync::Mutex;

use uuid::Uuid;

use crate::brain::agent::QueuedUserMessage;
use crate::brain::agent::service::MessageEnqueueCallback;
use crate::brain::agent::service::restart_recovery::{
    awaits_channel_route, claim_session, expect_channel_route, parked_count, test_guard,
};
use crate::brain::agent::service::session_routes::{
    Delivery, deliver_to_session, register_session_route, resolve_route,
};

type Seen = Arc<Mutex<Vec<(Uuid, String)>>>;

fn recorder() -> (MessageEnqueueCallback, Seen) {
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
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
        origin: crate::brain::agent::PushOrigin::Other,
    }
}

fn count(seen: &Seen) -> usize {
    seen.lock().map(|v| v.len()).unwrap_or(0)
}

#[test]
fn a_revived_channel_session_parks_instead_of_answering_the_local_surface() {
    let _guard = test_guard();
    let session = Uuid::new_v4();
    let (executing, seen) = recorder();

    expect_channel_route(session);
    resolve_route(session, &executing)(session, msg("bg done"));

    assert_eq!(
        count(&seen),
        0,
        "#1206: the completion went to the executing surface, which is a void on a \
         daemon and the wrong window under the TUI"
    );
    assert_eq!(parked_count(), 1, "#1206: and it was not parked either");
}

#[test]
fn an_ordinary_local_session_still_answers_the_executing_surface() {
    // The guard that keeps this from regressing TUI-local work: a session
    // nothing revived is genuinely local, and its completion must go straight
    // to the surface running it. No channel ever claims such a session, so
    // parking it would strand it forever.
    let _guard = test_guard();
    let session = Uuid::new_v4();
    let (executing, seen) = recorder();

    resolve_route(session, &executing)(session, msg("bg done"));

    assert_eq!(count(&seen), 1, "#1206: a local session must not be parked");
    assert_eq!(parked_count(), 0);
}

#[test]
fn claiming_the_session_flushes_what_parked_and_drops_the_mark() {
    let _guard = test_guard();
    let session = Uuid::new_v4();
    let (executing, executing_seen) = recorder();
    let (channel, channel_seen) = recorder();

    expect_channel_route(session);
    resolve_route(session, &executing)(session, msg("bg done"));
    assert!(awaits_channel_route(session));

    // The channel comes up and claims the session, exactly as the next
    // inbound message would.
    claim_session(session, &channel);

    assert_eq!(
        count(&channel_seen),
        1,
        "#1206: the parked completion did not reach the owning channel"
    );
    assert_eq!(count(&executing_seen), 0);
    assert!(
        !awaits_channel_route(session),
        "#1206: the mark outlived the gap it describes"
    );
}

#[test]
fn a_claimed_session_takes_the_fast_path_regardless_of_the_mark() {
    let _guard = test_guard();
    let session = Uuid::new_v4();
    let (executing, executing_seen) = recorder();
    let (channel, channel_seen) = recorder();

    // Mark first, then claim: a real route always wins over the expectation.
    expect_channel_route(session);
    register_session_route(session, channel);
    resolve_route(session, &executing)(session, msg("bg done"));

    assert_eq!(count(&channel_seen), 1, "#1206: the route must win");
    assert_eq!(count(&executing_seen), 0);
    assert_eq!(parked_count(), 0, "#1206: a routed session must not park");
}

#[test]
fn a_sub_agent_result_for_a_revived_session_parks_too() {
    // The sub-agent path reaches delivery through `deliver_to_session`, whose
    // fallback is the booting surface rather than an executing one. Same
    // reasoning, second door.
    let _guard = test_guard();
    let session = Uuid::new_v4();

    expect_channel_route(session);
    let outcome = deliver_to_session(session, msg("sub-agent result"), true);

    assert_eq!(
        outcome,
        Delivery::Parked,
        "#1206: the owning channel never saw it, so it must be held, not delivered"
    );
    assert_eq!(parked_count(), 1, "#1206: the result was not parked");
}
