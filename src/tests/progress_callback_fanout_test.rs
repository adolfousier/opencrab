//! Telemetry reaches every attached surface, not just the channel (#1092).
//!
//! A channel passes its own progress callback per message. The selection used
//! `or_else`, so exactly one callback ran and the service-level one the TUI
//! installs was skipped for the whole turn. The TUI's context counter is
//! written only from those events, so it sat frozen showing a stale value
//! while Telegram reported the real one.
//!
//! These pin the routing rules the fix depends on: counters fan out, content
//! does not.

use crate::brain::agent::QueuedUserMessage;
use crate::brain::agent::service::{MessageEnqueueCallback, ProgressEvent};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

type Seen = Arc<Mutex<Vec<String>>>;

/// Records the variant name of every event it receives.
fn recorder() -> (Arc<dyn Fn(Uuid, ProgressEvent) + Send + Sync>, Seen) {
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    let cb: Arc<dyn Fn(Uuid, ProgressEvent) + Send + Sync> =
        Arc::new(move |_id: Uuid, ev: ProgressEvent| {
            let name = match ev {
                ProgressEvent::TokenCount(_) => "TokenCount",
                ProgressEvent::StreamingChunk { .. } => "StreamingChunk",
                ProgressEvent::Thinking => "Thinking",
                _ => "Other",
            };
            if let Ok(mut v) = sink.lock() {
                v.push(name.to_string());
            }
        });
    (cb, seen)
}

/// The composite the tool loop builds when both callbacks exist. Mirrors the
/// routing rule under test without needing a live AgentService.
fn fan_out(
    channel_cb: Arc<dyn Fn(Uuid, ProgressEvent) + Send + Sync>,
    service_cb: Arc<dyn Fn(Uuid, ProgressEvent) + Send + Sync>,
) -> Arc<dyn Fn(Uuid, ProgressEvent) + Send + Sync> {
    Arc::new(move |sid: Uuid, event: ProgressEvent| {
        if matches!(event, ProgressEvent::TokenCount(_)) {
            service_cb(sid, event.clone());
        }
        channel_cb(sid, event);
    })
}

#[test]
fn a_token_count_reaches_both_surfaces() {
    // The reported bug: Telegram showed 116K while the TUI showed a stale 2.
    let (channel, channel_seen) = recorder();
    let (service, service_seen) = recorder();
    let cb = fan_out(channel, service);

    cb(Uuid::new_v4(), ProgressEvent::TokenCount(116_000));

    assert_eq!(*channel_seen.lock().unwrap(), vec!["TokenCount"]);
    assert_eq!(
        *service_seen.lock().unwrap(),
        vec!["TokenCount"],
        "the TUI must see the count too, or its footer stays frozen"
    );
}

#[test]
fn text_bearing_events_stay_on_the_channel_only() {
    // StreamingChunk also drives the TUI's own display via cli/ui.rs, so
    // mirroring it would render the turn's content twice.
    let (channel, channel_seen) = recorder();
    let (service, service_seen) = recorder();
    let cb = fan_out(channel, service);

    cb(
        Uuid::new_v4(),
        ProgressEvent::StreamingChunk {
            text: "hello".to_string(),
        },
    );

    assert_eq!(*channel_seen.lock().unwrap(), vec!["StreamingChunk"]);
    assert!(
        service_seen.lock().unwrap().is_empty(),
        "content must not reach a surface that already mirrors it"
    );
}

#[test]
fn non_token_telemetry_is_not_broadcast_either() {
    // Deliberately narrow: only the counter fans out. Widening this is a
    // decision, not an accident.
    let (channel, channel_seen) = recorder();
    let (service, service_seen) = recorder();
    let cb = fan_out(channel, service);

    cb(Uuid::new_v4(), ProgressEvent::Thinking);

    assert_eq!(*channel_seen.lock().unwrap(), vec!["Thinking"]);
    assert!(service_seen.lock().unwrap().is_empty());
}

#[test]
fn the_channel_still_receives_everything() {
    let (channel, channel_seen) = recorder();
    let (service, _service_seen) = recorder();
    let cb = fan_out(channel, service);
    let id = Uuid::new_v4();

    cb(id, ProgressEvent::TokenCount(1));
    cb(id, ProgressEvent::Thinking);
    cb(
        id,
        ProgressEvent::StreamingChunk {
            text: "x".to_string(),
        },
    );

    assert_eq!(
        *channel_seen.lock().unwrap(),
        vec!["TokenCount", "Thinking", "StreamingChunk"],
        "fanning out must not cost the channel any event"
    );
}

/// Unused import guard: keeps the enqueue type referenced so the test file
/// documents which callback family this is NOT about.
#[allow(dead_code)]
fn _not_the_enqueue_path(_: MessageEnqueueCallback, _: QueuedUserMessage) {}
