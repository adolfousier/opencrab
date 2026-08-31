//! Quiet delivery engine (Notifications v2, fork #50).
//!
//! Holds a deferred notification until the target session has been quiet
//! for a window: no active turn, and `quiet_for` elapsed since the last
//! observed turn end. Any turn activity restarts the clock. A starvation
//! cap (`max_delay`) forces delivery — queueing into the running turn —
//! so a permanently busy session cannot defer a notice forever.
//!
//! The quiet clock is PER TARGET, stored in the registry (not on a
//! watcher's stack): every watcher for a target reads and restarts the
//! same clock, and a batch sweep sees every entry's real window.
//!
//! Release is a per-target BATCH: when one deferred entry becomes due,
//! every entry for the same target that is due under the same clock drains
//! together — the first wakes the session, the rest ride the woken turn's
//! boundaries. Route (and the fork #17/#19 channel-ownership gate)
//! re-evaluates at fire time via `deliver_to_session`.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use uuid::Uuid;

use super::session_routes::{Delivery, deliver_to_session, turn_probe};
use crate::brain::agent::QueuedUserMessage;

/// How often a watcher re-probes the target's turn state.
pub const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// A banked notification awaiting its quiet window.
pub struct DeferredNotify {
    pub msg: QueuedUserMessage,
    pub quiet_for: Duration,
    pub max_delay: Duration,
    /// Birth of THIS entry — the starvation cap runs per entry, so a
    /// late-arriving entry is not starved by an older one's clock.
    pub created_at: Instant,
}

/// Per-target state: the shared quiet clock plus that target's entries.
struct TargetState {
    /// Last instant the target was observed mid-turn (or the first defer
    /// for this target — conservative: a fresh target waits a full window).
    last_busy: Instant,
    entries: HashMap<Uuid, DeferredNotify>,
}

fn registry() -> &'static Mutex<HashMap<Uuid, TargetState>> {
    static CELL: OnceLock<Mutex<HashMap<Uuid, TargetState>>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Bank a notification for quiet delivery; returns its id (the cancel/list
/// handle). The watcher task lives in the tokio runtime that spawned it —
/// the same process that owns the target session, so probes stay valid.
pub fn defer_quiet(
    target: Uuid,
    msg: QueuedUserMessage,
    quiet_for: Duration,
    max_delay: Duration,
) -> Uuid {
    let id = Uuid::new_v4();
    let entry = DeferredNotify {
        msg,
        quiet_for,
        max_delay,
        created_at: Instant::now(),
    };
    if let Ok(mut guard) = registry().lock() {
        let state = guard.entry(target).or_insert_with(|| TargetState {
            last_busy: Instant::now(),
            entries: HashMap::new(),
        });
        state.entries.insert(id, entry);
    }
    tokio::spawn(watch(id, target, POLL_INTERVAL));
    id
}

/// Cancel a deferred notification before it fires. `false` = already
/// delivered, already cancelled, or unknown id (`too_late` in the v2
/// verdict vocabulary).
/// No lib consumer yet — the `cancel` action rides a later enum value
/// (fork #50); the id returned by `defer_quiet` is the handle.
#[cfg_attr(not(test), expect(dead_code))]
pub fn cancel_deferred(id: Uuid) -> bool {
    let Ok(mut guard) = registry().lock() else {
        return false;
    };
    for state in guard.values_mut() {
        if state.entries.remove(&id).is_some() {
            guard.retain(|_, s| !s.entries.is_empty());
            return true;
        }
    }
    false
}

/// Due predicate, pure so the window math is pinable by tests.
///
/// Due when the starvation cap is hit regardless of turn state (forced
/// delivery), or when the target is idle AND the quiet window has elapsed.
pub fn is_due(
    mid_turn: bool,
    quiet_elapsed: Duration,
    total_elapsed: Duration,
    quiet_for: Duration,
    max_delay: Duration,
) -> bool {
    if total_elapsed >= max_delay {
        return true;
    }
    !mid_turn && quiet_elapsed >= quiet_for
}

/// Watch one deferred entry; on the first due sweep for its target, drain
/// every due entry for that target in one batch.
async fn watch(id: Uuid, target: Uuid, poll: Duration) {
    loop {
        tokio::time::sleep(poll).await;
        let now = Instant::now();
        let mid_turn = turn_probe(target).is_some_and(|probe| probe());

        let Some(batch) = sweep(id, target, now, mid_turn) else {
            continue; // not due yet (or own entry already gone)
        };
        release_batch(target, batch, mid_turn);
        return;
    }
}

/// Check + collect under one lock: due-ness of the firing entry and of
/// every same-target entry under the SHARED clock. Returns `None` while
/// waiting; `Some(batch)` exactly once, with the entries removed from the
/// registry (empty target states pruned). An already-vanished firing entry
/// yields `Some(vec![])` — the watcher stops without delivering.
fn sweep(
    firing_id: Uuid,
    target: Uuid,
    now: Instant,
    mid_turn: bool,
) -> Option<Vec<(Uuid, DeferredNotify)>> {
    let mut batch: Vec<(Uuid, DeferredNotify)> = Vec::new();
    let fired = {
        let Ok(mut guard) = registry().lock() else {
            return None;
        };
        let Some(state) = guard.get_mut(&target) else {
            return Some(Vec::new()); // target state gone — everything delivered/cancelled
        };
        if mid_turn {
            // Any turn activity restarts the quiet clock.
            state.last_busy = now;
        }
        let quiet_elapsed = now.duration_since(state.last_busy);
        let due = |e: &DeferredNotify| {
            is_due(
                mid_turn,
                quiet_elapsed,
                now.duration_since(e.created_at),
                e.quiet_for,
                e.max_delay,
            )
        };
        if let Some(entry) = state.entries.get(&firing_id)
            && !due(entry)
        {
            return None;
        }
        let ids: Vec<Uuid> = state
            .entries
            .iter()
            .filter(|(eid, e)| *eid == &firing_id || due(e))
            .map(|(eid, _)| *eid)
            .collect();
        for eid in ids {
            if let Some(entry) = state.entries.remove(&eid) {
                batch.push((eid, entry));
            }
        }
        if state.entries.is_empty() {
            guard.remove(&target);
        }
        !batch.is_empty()
    };
    fired.then_some(batch)
}

/// Deliver a drained batch. The first entry wakes (`interrupt=false`)
/// unless the target is mid-turn at fire time — then ALL ride the running
/// turn (`interrupt=true`, the starvation cap's forced delivery);
/// subsequent entries always ride. The channel-ownership gate
/// re-evaluates per delivery inside `deliver_to_session`.
fn release_batch(target: Uuid, batch: Vec<(Uuid, DeferredNotify)>, mid_turn: bool) {
    for (idx, (id, entry)) in batch.into_iter().enumerate() {
        let interrupt = mid_turn || idx > 0;
        let outcome = deliver_to_session(target, entry.msg, interrupt);
        tracing::info!(
            target: "quiet_delivery",
            id = %id,
            session = %target,
            position = idx,
            interrupt,
            quiet_for_secs = entry.quiet_for.as_secs_f64(),
            max_delay_secs = entry.max_delay.as_secs_f64(),
            waited_secs = entry.created_at.elapsed().as_secs_f64(),
            "quiet batch release: {}",
            outcome_state(&outcome)
        );
    }
}

/// External v2 state name for a delivery outcome (diagnostics only here;
/// the tool-level mapping lives in the session_notify tool).
fn outcome_state(delivery: &Delivery) -> &'static str {
    match delivery {
        Delivery::Delivered => "delivered",
        Delivery::Parked => "queued",
        Delivery::Redirected { .. } => "redirected",
        Delivery::RefusedInFlight { .. } | Delivery::NoRoute => "refused",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

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
        super::super::session_routes::register_session_route(
            session,
            Arc::new(move |_id, queued| {
                *sink.lock().unwrap() = Some(queued);
            }),
        );
        super::super::session_routes::register_turn_probe(session, Arc::new(|| false));

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
        super::super::session_routes::register_session_route(
            session,
            Arc::new(move |_id, queued| {
                *sink.lock().unwrap() = Some(queued);
            }),
        );
        super::super::session_routes::register_turn_probe(session, Arc::new(|| true));

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
        super::super::session_routes::register_session_route(
            session,
            Arc::new(move |_id, queued| {
                sink.lock().unwrap().push(queued);
            }),
        );
        super::super::session_routes::register_turn_probe(session, Arc::new(|| false));

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
}
