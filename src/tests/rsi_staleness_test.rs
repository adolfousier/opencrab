//! Tests for the Mission Control RSI staleness line (#469): a dark RSI must
//! read differently from a healthy one, and the warning fires only when
//! activity kept recording while RSI slept.

use crate::brain::mission_control::staleness::{RSI_STALE_AFTER_SECS, rsi_staleness_line};

const NOW: i64 = 1_800_000_000;

#[test]
fn healthy_recent_run_reads_plain() {
    let line = rsi_staleness_line(NOW, Some(NOW - 2 * 3600), 500);
    assert_eq!(line, "RSI last run 2h 0m ago");
    assert!(!line.contains('⚠'));
}

#[test]
fn stale_with_recorded_events_warns() {
    // The live incident shape: three days dark while thousands of tool
    // events recorded.
    let line = rsi_staleness_line(NOW, Some(NOW - (3 * 86_400 + 2 * 3600)), 12_245);
    assert!(line.starts_with("⚠️ RSI stale: last run 3d 2h ago"));
    assert!(line.contains("12245 tool event(s) since"));
}

#[test]
fn stale_but_idle_ledger_stays_calm() {
    // Nothing happened since the last run: RSI has nothing to process, so
    // old age alone is not a warning.
    let line = rsi_staleness_line(NOW, Some(NOW - 5 * 86_400), 0);
    assert!(!line.contains('⚠'));
    assert!(line.contains("5d 0h ago"));
}

#[test]
fn boundary_is_twelve_hours() {
    let just_under = rsi_staleness_line(NOW, Some(NOW - RSI_STALE_AFTER_SECS + 60), 10);
    assert!(!just_under.contains('⚠'));
    let just_over = rsi_staleness_line(NOW, Some(NOW - RSI_STALE_AFTER_SECS - 60), 10);
    assert!(just_over.contains('⚠'));
}

#[test]
fn never_ran_states_it() {
    assert_eq!(rsi_staleness_line(NOW, None, 0), "RSI has never run");
    let with_events = rsi_staleness_line(NOW, None, 42);
    assert!(with_events.contains('⚠'));
    assert!(with_events.contains("42 tool event(s) recorded"));
}

#[test]
fn sub_hour_and_sub_minute_ages_humanize() {
    assert!(rsi_staleness_line(NOW, Some(NOW - 42 * 60), 3).contains("42m ago"));
    assert!(rsi_staleness_line(NOW, Some(NOW - 20), 0).contains("<1m ago"));
}
