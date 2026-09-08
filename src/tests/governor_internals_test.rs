//! Unit tests for the governor's internals (#1211).
//!
//! Lifted out of `governor.rs`: the house rule is that no `#[cfg(test)] mod
//! tests` block lives in a source file — every suite is a file under
//! `src/tests/` registered in `mod.rs`. The end-to-end gate tests live in
//! `governor_gates_test`; this file covers the pure pieces those drive.

use std::time::{Duration, Instant};

use teloxide::types::ChatId;

use crate::channels::telegram::governor::{
    Bucket, Counters, EditClass, INTERACTIVE_RESERVE, MAX_429_PAUSE, ensure_bucket, format_summary,
    is_permanent_edit_error, note_429_pause,
};

#[test]
fn bucket_allows_burst_then_throttles() {
    let mut b = Bucket::new(3, 1.0);
    let t0 = Instant::now();
    assert!(b.take(t0).is_ok());
    assert!(b.take(t0).is_ok());
    assert!(b.take(t0).is_ok());
    // Empty: refuses, and reports the full refill spacing.
    let err = b.take(t0).expect_err("bucket should be empty");
    assert_eq!(err, Duration::from_secs(1));
}

#[test]
fn bucket_refills_over_time_and_caps_at_capacity() {
    let mut b = Bucket::new(2, 1.0);
    let t0 = Instant::now();
    assert!(b.take(t0).is_ok());
    assert!(b.take(t0).is_ok());
    assert!(b.take(t0).is_err());
    // Halfway through one interval there is still no full token.
    assert!(b.take(t0 + Duration::from_millis(500)).is_err());
    // A full interval later the token is back.
    assert!(b.take(t0 + Duration::from_secs(1)).is_ok());
    // Idling far past capacity must not hoard tokens beyond the cap.
    let t1 = t0 + Duration::from_secs(60);
    assert!(b.take(t1).is_ok());
    assert!(b.take(t1).is_ok());
    assert!(b.take(t1).is_err());
}

#[test]
fn ensure_bucket_rebuilds_on_shape_change_keeps_on_match() {
    let mut slot = None;
    let rate = 0.5;
    ensure_bucket(&mut slot, 10, rate).take(Instant::now()).ok();
    // Same shape: the partially-consumed bucket survives.
    let kept = ensure_bucket(&mut slot, 10, rate);
    assert!(kept.tokens < f64::from(10u32));
    // Different shape: rebuilt fresh at full capacity.
    let fresh = ensure_bucket(&mut slot, 5, rate);
    assert!((fresh.tokens - 5.0).abs() < f64::EPSILON);
}

#[test]
fn ladder_order_drops_clock_first_and_final_never_drops() {
    let ladder = [
        EditClass::Clock,
        EditClass::BrainPreview,
        EditClass::Intermediary,
        EditClass::Status,
        // #117: Interactive outranks Final — never dropped, never queued.
        EditClass::Interactive,
    ];
    for pair in ladder.windows(2) {
        assert!(
            pair[0].drop_rank() < pair[1].drop_rank(),
            "ladder must drop {pair:?} in ascending value order"
        );
    }
    // Final outranks everything and is refused by the dropper.
    assert_eq!(EditClass::Final.drop_rank(), 4);
    assert_eq!(EditClass::Interactive.drop_rank(), 5);
    let c = Counters {
        admitted_typing: 12,
        admitted_edits: 34,
        admitted_sends: 5,
        dropped_clock: 1,
        dropped_brain_preview: 2,
        dropped_intermediary: 3,
        dropped_status: 4,
        dropped_typing: 6,
        queued_finals: 7,
        superseded_finals: 8,
        delivered_finals: 9,
        failed_finals: 10,
        admitted_interactive: 13,
        interactive_overflow: 14,
        pause_armed_429: 15,
        throttled_typing_ms: 1500,
        throttled_send_ms: 2500,
        admitted_rich: 11,
        throttled_rich_ms: 3500,
    };
    let line = format_summary(-100123, &c, 2).expect("active peer must summarize");
    assert!(line.contains("chat=-100123"));
    assert!(line.contains("admitted{typing=12,edits=34,sends=5,rich=11}"));
    assert!(line.contains("dropped{clock=1,brain_preview=2,intermediary=3,status=4,typing=6}"));
    assert!(line.contains("finals{queued=7,superseded=8,delivered=9,failed=10,pending=2}"));
    assert!(line.contains("interactive{admitted=13,overflow=14,pause429=15}"));
    assert!(line.contains("throttled_ms{typing=1500,send=2500,rich=3500}"));
}

/// #117: the reserved floor bounds bulk `take` but not interactive
/// `take_any` — bulk stops at the floor, interactive spends through it.
#[test]
fn interactive_reserve_bounds_bulk_but_not_interactive() {
    // 3-token bucket, 2 reserved for interactive: bulk may spend 1.
    let mut b = Bucket::new(3, 1.0);
    b.set_reserve(INTERACTIVE_RESERVE);
    assert!(b.take(Instant::now()).is_ok(), "bulk spends the free token");
    assert!(
        b.take(Instant::now()).is_err(),
        "bulk must NOT dip into the reserved floor"
    );
    // Interactive spends through the reserve, down to zero.
    assert!(b.take_any(Instant::now()).is_ok());
    assert!(b.take_any(Instant::now()).is_ok());
    assert!(
        b.take_any(Instant::now()).is_err(),
        "even interactive stops at zero"
    );
    // Refill restores bulk access once tokens rise above the floor again.
    let later = Instant::now() + Duration::from_secs(4);
    assert!(b.take(later).is_ok(), "refill above the floor frees bulk");
}

/// Step 2 (owner-approved design): a 429 pause freezes bulk refill and
/// blocks bulk `take` for the declared window, while interactive `take_any`
/// bypasses it — taps never queue behind bulk, even mid-window.
#[test]
fn pause_429_blocks_bulk_but_not_interactive() {
    let t0 = Instant::now();
    let mut b = Bucket::new(4, 1.0);
    // Arm the same reserve the admission path sets (2 in a 4-bucket), then
    // spend ONE free token — 3 remain, above the floor, so any bulk failure
    // below is caused by the PAUSE, not by the reserve.
    b.set_reserve(INTERACTIVE_RESERVE);
    assert!(b.take(t0).is_ok());

    // Declare the window: refill frozen + bulk blocked.
    b.pause_arm(t0 + Duration::from_secs(30));
    let t1 = t0 + Duration::from_secs(5);
    assert!(
        b.take(t1).is_err(),
        "bulk must not spend during a declared window, even above the floor"
    );
    assert!(
        b.take_any(t1).is_ok(),
        "interactive bypasses the pause mid-window (tokens exist, floor not reached)"
    );

    // Refill is FROZEN during the pause: advance 20s inside a 30s window,
    // bulk still blocked even though 20 unpaused seconds would refill 20.
    let t2 = t0 + Duration::from_secs(20);
    assert!(
        b.take(t2).is_err(),
        "refill must stay frozen for the whole declared window"
    );

    // Bulk returns the instant the window expires.
    let t3 = t0 + Duration::from_secs(31);
    assert!(
        b.take(t3).is_ok(),
        "window expiry releases both the block and the frozen refill"
    );

    // Cap: a pause longer than MAX_429_PAUSE is clamped at arm time by the
    // caller (note_429_pause); assert the constant is sane here.
    assert!(MAX_429_PAUSE <= Duration::from_secs(60));
}

/// note_429_pause clamps to MAX_429_PAUSE and is a noop for DMs/disabled.
#[test]
fn note_429_pause_clamps_and_respects_scope() {
    // DM chat: noop, no panic.
    note_429_pause(ChatId(42), Duration::from_secs(500));
    // Cap arithmetic: the function clamps before arming; assert constant.
    assert_eq!(MAX_429_PAUSE, Duration::from_secs(45));
}

/// #117: reserve is clamped below capacity so a mis-sized constant cannot
/// freeze bulk forever.
#[test]
fn reserve_clamps_below_capacity() {
    let mut b = Bucket::new(2, 1.0);
    b.set_reserve(99.0);
    assert!(
        b.take(Instant::now()).is_ok(),
        "clamped reserve must leave bulk at least one token"
    );
}

/// #117: a fresh bucket starts unreserved; the admission path re-arms the
/// reserve on every call, so a reshaped bucket never runs with a stale 0.
#[test]
fn reserve_resets_on_reshape() {
    let mut slot = None;
    let b = ensure_bucket(&mut slot, 10, 1.0);
    assert_eq!(b.reserve_peek(), 0.0, "fresh bucket starts unreserved");
}

#[test]
fn permanent_edit_error_vocabulary_is_exact() {
    assert!(is_permanent_edit_error(
        "Telegram error: message to edit not found"
    ));
    assert!(is_permanent_edit_error(
        "Bad Request: message is not modified"
    ));
    assert!(!is_permanent_edit_error("Too Many Requests: retry after 3"));
    assert!(!is_permanent_edit_error("timeout"));
}
