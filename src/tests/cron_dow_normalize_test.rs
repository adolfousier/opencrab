//! Unix day-of-week `0` must parse as Sunday (#1024).
//!
//! Unix cron numbers days 0-6 with Sunday at 0; the `cron` crate numbers them
//! 1-7 with Sunday at 7. `0 4 * * 0` — the way every crontab writes "Sunday at
//! 04:00" — failed to parse entirely, so the built-in dedup-scan job never ran
//! once and logged a parse failure every minute for its whole lifetime.
//!
//! That job is what catches the same rule being written into two brain files,
//! which is why a live workspace accumulated 63 near-duplicate rules and a
//! permissions rule that drifted into two contradictory statements (#1017).

use crate::cron::schedule_util::{next_run_utc, normalize_day_of_week};
use chrono::Utc;

/// The expression that shipped broken.
#[test]
fn sunday_as_zero_now_parses() {
    assert!(
        next_run_utc("0 4 * * 0", chrono_tz::UTC, Utc::now()).is_some(),
        "`0 4 * * 0` is how Unix cron spells Sunday and must schedule"
    );
}

/// Sunday-as-7 keeps working, so anyone who worked around this is unaffected.
#[test]
fn sunday_as_seven_still_parses() {
    assert!(next_run_utc("0 4 * * 7", chrono_tz::UTC, Utc::now()).is_some());
}

/// Only the day-of-week field is rewritten.
#[test]
fn only_whole_zero_tokens_become_seven() {
    assert_eq!(normalize_day_of_week("0"), "7");
    assert_eq!(normalize_day_of_week("*"), "*");
    // A minute-style value must never be mangled if it reaches this field.
    assert_eq!(normalize_day_of_week("10"), "10");
    assert_eq!(normalize_day_of_week("30"), "30");
}

/// Lists, ranges and steps survive the rewrite.
#[test]
fn compound_day_of_week_expressions_survive() {
    assert_eq!(normalize_day_of_week("0,3"), "7,3");
    assert_eq!(normalize_day_of_week("1-5"), "1-5");
    assert_eq!(normalize_day_of_week("0-3"), "7-3");
    assert_eq!(normalize_day_of_week("*/2"), "*/2");
    assert_eq!(normalize_day_of_week("MON"), "MON");
}

/// A weekday expression with no Sunday is untouched end to end.
#[test]
fn weekday_schedules_are_unaffected() {
    assert!(next_run_utc("0 9 * * 1-5", chrono_tz::UTC, Utc::now()).is_some());
    assert!(next_run_utc("*/15 * * * *", chrono_tz::UTC, Utc::now()).is_some());
}

/// Genuinely malformed expressions still fail rather than silently scheduling.
#[test]
fn malformed_expressions_still_fail() {
    assert!(next_run_utc("not a cron", chrono_tz::UTC, Utc::now()).is_none());
    assert!(next_run_utc("0 4 * *", chrono_tz::UTC, Utc::now()).is_none());
}
