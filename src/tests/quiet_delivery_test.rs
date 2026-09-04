//! Quiet delivery engine (fork #50): due rule, starvation cap, batch release through the session route.

use crate::brain::agent::QueuedUserMessage;
use crate::brain::agent::service::quiet_delivery::*;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

fn msg() -> QueuedUserMessage {
    QueuedUserMessage {
        context_text: "[session-notify from=x]\n\nbody".to_string(),
        display_text: "notify".to_string(),
        origin: crate::brain::agent::PushOrigin::SessionNotify,
        bg_meta: None,
    }
}

#[test]
fn due_requires_idle_and_quiet_window() {
    let quiet = Duration::from_secs(60);
    let cap = Duration::from_secs(1800);
    assert!(!is_due(
        true,
        Duration::from_secs(120),
        Duration::from_secs(120),
        quiet,
        cap
    ));
    assert!(!is_due(
        false,
        Duration::from_secs(10),
        Duration::from_secs(10),
        quiet,
        cap
    ));
    assert!(is_due(
        false,
        Duration::from_secs(61),
        Duration::from_secs(61),
        quiet,
        cap
    ));
}

#[test]
fn starvation_cap_forces_due_even_mid_turn() {
    let quiet = Duration::from_secs(60);
    let cap = Duration::from_secs(1800);
    assert!(is_due(true, Duration::ZERO, cap, quiet, cap));
}

#[tokio::test]
async fn registry_cancel_bookkeeping() {
    let target = Uuid::new_v4();
    let id = defer_quiet(
        target,
        msg(),
        Duration::from_secs(3600),
        Duration::from_secs(7200),
    );
    assert!(cancel_deferred(id));
    assert!(!cancel_deferred(id), "second cancel = too_late");
}

// Integration: quiet window elapses on an idle target, the watcher
// batch-releases through the registered route.
#[tokio::test]
async fn quiet_release_delivers_after_window() {
    let session = Uuid::new_v4();
    let captured: Arc<std::sync::Mutex<Option<QueuedUserMessage>>> =
        Arc::new(std::sync::Mutex::new(None));
    let sink = captured.clone();
    crate::brain::agent::service::session_routes::register_session_route(
        session,
        Arc::new(move |_id, queued| {
            *sink.lock().unwrap() = Some(queued);
        }),
    );
    crate::brain::agent::service::session_routes::register_turn_probe(session, Arc::new(|| false));

    let id = defer_quiet(
        session,
        msg(),
        Duration::from_millis(50),
        Duration::from_secs(30),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while captured.lock().unwrap().is_none() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        captured.lock().unwrap().is_some(),
        "quiet notification should deliver after the window"
    );
    assert!(!cancel_deferred(id), "delivered entry can no longer cancel");
}

// Starvation cap: a permanently busy target still receives at
// max_delay, riding the running turn (interrupt=true).
#[tokio::test]
async fn starvation_cap_forces_delivery_into_busy_turn() {
    let session = Uuid::new_v4();
    let captured: Arc<std::sync::Mutex<Option<QueuedUserMessage>>> =
        Arc::new(std::sync::Mutex::new(None));
    let sink = captured.clone();
    crate::brain::agent::service::session_routes::register_session_route(
        session,
        Arc::new(move |_id, queued| {
            *sink.lock().unwrap() = Some(queued);
        }),
    );
    crate::brain::agent::service::session_routes::register_turn_probe(session, Arc::new(|| true));

    let _id = defer_quiet(
        session,
        msg(),
        Duration::from_secs(300),
        Duration::from_millis(80),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while captured.lock().unwrap().is_none() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        captured.lock().unwrap().is_some(),
        "max_delay must force delivery even mid-turn"
    );
}

// Batch: two entries banked back-to-back drain together on the first
// due sweep — one wake, two deliveries.
#[tokio::test]
async fn batch_release_drains_same_target_together() {
    let session = Uuid::new_v4();
    let captured: Arc<std::sync::Mutex<Vec<QueuedUserMessage>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = captured.clone();
    crate::brain::agent::service::session_routes::register_session_route(
        session,
        Arc::new(move |_id, queued| {
            sink.lock().unwrap().push(queued);
        }),
    );
    crate::brain::agent::service::session_routes::register_turn_probe(session, Arc::new(|| false));

    let _a = defer_quiet(
        session,
        msg(),
        Duration::from_millis(60),
        Duration::from_secs(30),
    );
    let _b = defer_quiet(
        session,
        msg(),
        Duration::from_millis(60),
        Duration::from_secs(30),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while captured.lock().unwrap().len() < 2 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        captured.lock().unwrap().len(),
        2,
        "both same-target entries drain in one batch"
    );
}
