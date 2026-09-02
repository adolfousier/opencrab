//! A wake waits for its channel's transport instead of dropping (#1242).
//!
//! The boot race: the recovery pass and the channel's own connect run
//! concurrently with nothing ordering them, so a wake regularly asked for a
//! transport seconds before it existed. Every adapter answered with a warn
//! and a return, which is a permanent loss — observed across ~21 daemon
//! restarts in 48h, once losing a background result for a whole day.
//!
//! Time is paused in these tests, so the grace window is asserted exactly
//! rather than waited out.

use crate::channels::transport_ready::{CONNECT_GRACE, await_transport};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// A transport slot the channel fills from its own task, which is what the
/// adapters are really polling: a mutable `Option`, not a future.
#[derive(Clone, Default)]
struct Slot(Arc<std::sync::Mutex<Option<&'static str>>>);

impl Slot {
    fn connect(&self, name: &'static str) {
        *self.0.lock().expect("slot poisoned") = Some(name);
    }
    fn get(&self) -> Option<&'static str> {
        *self.0.lock().expect("slot poisoned")
    }
}

#[tokio::test(start_paused = true)]
async fn an_already_connected_transport_costs_no_wait() {
    let slot = Slot::default();
    slot.connect("bot");

    let started = tokio::time::Instant::now();
    let got = await_transport("telegram", uuid::Uuid::nil(), || async { slot.get() }).await;

    assert_eq!(got, Some("bot"));
    assert_eq!(
        started.elapsed(),
        Duration::ZERO,
        "the ordinary case must be exactly as fast as the bare check it replaced"
    );
}

#[tokio::test(start_paused = true)]
async fn a_late_connect_is_picked_up_rather_than_dropped() {
    // The issue's acceptance criterion: the channel connects AFTER the
    // recovery pass has already asked for it. Zero drops.
    let slot = Slot::default();
    let filler = slot.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        filler.connect("bot");
    });

    let got = await_transport("telegram", uuid::Uuid::nil(), || async { slot.get() }).await;

    assert_eq!(got, Some("bot"), "a wake was dropped on a late connect");
}

#[tokio::test(start_paused = true)]
async fn a_late_connect_is_served_within_the_grace_window() {
    let slot = Slot::default();
    let filler = slot.clone();
    tokio::spawn(async move {
        tokio::time::sleep(CONNECT_GRACE - Duration::from_secs(1)).await;
        filler.connect("bot");
    });

    let started = tokio::time::Instant::now();
    let got = await_transport("telegram", uuid::Uuid::nil(), || async { slot.get() }).await;

    assert_eq!(got, Some("bot"));
    assert!(
        started.elapsed() <= CONNECT_GRACE,
        "a connect inside the window was still missed after {:?}",
        started.elapsed()
    );
}

#[tokio::test(start_paused = true)]
async fn a_connect_follows_closely_rather_than_on_the_next_second() {
    // The poll interval is the delay between a channel connecting and its
    // wake going out. A whole second of it would be visible on every boot.
    let slot = Slot::default();
    let filler = slot.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        filler.connect("bot");
    });

    let started = tokio::time::Instant::now();
    await_transport("telegram", uuid::Uuid::nil(), || async { slot.get() }).await;

    assert!(
        started.elapsed() < Duration::from_secs(1),
        "wake trailed its connect by {:?}",
        started.elapsed()
    );
}

#[tokio::test(start_paused = true)]
async fn a_channel_that_never_connects_gives_up_bounded() {
    // A channel not configured in this run never connects. Waiting forever
    // would leak one task per wake, so the window has to end.
    let slot = Slot::default();

    let started = tokio::time::Instant::now();
    let got = await_transport("telegram", uuid::Uuid::nil(), || async { slot.get() }).await;

    assert_eq!(got, None);
    assert!(
        started.elapsed() >= CONNECT_GRACE,
        "gave up after only {:?}, short of the grace window",
        started.elapsed()
    );
    assert!(
        started.elapsed() < CONNECT_GRACE * 2,
        "overshot the grace window by more than a whole window: {:?}",
        started.elapsed()
    );
}

#[tokio::test(start_paused = true)]
async fn readiness_is_re_read_every_poll() {
    // `lookup` is re-run per poll rather than awaited once, because the thing
    // being watched is a slot the channel mutates, not a future that resolves.
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let slot = Slot::default();
    let filler = slot.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(2)).await;
        filler.connect("bot");
    });

    let got = await_transport("telegram", uuid::Uuid::nil(), || {
        counter.fetch_add(1, Ordering::SeqCst);
        async { slot.get() }
    })
    .await;

    assert_eq!(got, Some("bot"));
    assert!(
        calls.load(Ordering::SeqCst) > 1,
        "the slot was read once and cached, so a later connect could never be seen"
    );
}

#[test]
fn the_two_startup_flush_paths_share_one_window() {
    // The paths raced because they had different unstated readiness
    // assumptions. Same window by construction is what makes the ordering
    // deterministic instead of incidental.
    assert_eq!(
        CONNECT_GRACE,
        crate::brain::agent::service::restart_recovery::ROUTE_GRACE,
        "the wake wait and the parked-report flush drifted apart again"
    );
}
