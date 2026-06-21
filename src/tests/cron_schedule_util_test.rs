//! Timezone- and day-of-week-aware cron schedule helpers.
//!
//! These pin the two things the agent kept getting wrong:
//!   * day-of-week is 1-7 = Sun-Sat in the `cron` crate (1=Sunday), and day
//!     NAMES map to the right weekday; and
//!   * the job's timezone is actually honored — "09:00 America/New_York"
//!     fires at the correct UTC instant, DST included — which was a silent
//!     bug before (everything ran in UTC).

use crate::cron::{next_run_utc, parse_timezone, upcoming_in_tz};
use chrono::{Datelike, TimeZone, Timelike, Utc, Weekday};
use chrono_tz::UTC;

// ── day-of-week encoding (the footgun) ──────────────────────────────────────

#[test]
fn numeric_dow_is_sunday_first() {
    // After a fixed reference instant, assert the weekday of the next run.
    // 1 = Sunday, 2 = Monday in the cron crate (chrono's number_from_sunday).
    let after = Utc.with_ymd_and_hms(2026, 6, 17, 12, 0, 0).unwrap(); // a Wednesday
    let sunday = upcoming_in_tz("0 0 * * 1", UTC, 1, after);
    let monday = upcoming_in_tz("0 0 * * 2", UTC, 1, after);
    assert_eq!(sunday[0].weekday(), Weekday::Sun, "dow=1 must be Sunday");
    assert_eq!(monday[0].weekday(), Weekday::Mon, "dow=2 must be Monday");
}

#[test]
fn dow_zero_is_rejected() {
    // Standard Unix uses 0 for Sunday; this crate rejects 0 outright. The
    // tool/CLI surface that as an error — here we just confirm no runs come
    // back (malformed → empty), so we never silently store a broken job.
    let after = Utc.with_ymd_and_hms(2026, 6, 17, 12, 0, 0).unwrap();
    assert!(
        upcoming_in_tz("0 0 * * 0", UTC, 1, after).is_empty(),
        "dow=0 is invalid in this crate and must yield no runs"
    );
}

#[test]
fn day_names_map_to_correct_weekday() {
    let after = Utc.with_ymd_and_hms(2026, 6, 17, 12, 0, 0).unwrap();
    for (expr, want) in [
        ("0 0 * * Sun", Weekday::Sun),
        ("0 0 * * Mon", Weekday::Mon),
        ("0 0 * * Fri", Weekday::Fri),
        ("0 0 * * Sat", Weekday::Sat),
    ] {
        let runs = upcoming_in_tz(expr, UTC, 1, after);
        assert_eq!(runs[0].weekday(), want, "{expr} must fall on {want:?}");
    }
}

#[test]
fn weekday_range_name_excludes_weekend() {
    let after = Utc.with_ymd_and_hms(2026, 6, 17, 12, 0, 0).unwrap();
    for run in upcoming_in_tz("0 9 * * Mon-Fri", UTC, 10, after) {
        let wd = run.weekday();
        assert!(
            wd != Weekday::Sat && wd != Weekday::Sun,
            "Mon-Fri must never land on a weekend, got {wd:?}"
        );
        assert_eq!(run.hour(), 9);
    }
}

// ── timezone is honored (the silent bug) ────────────────────────────────────

#[test]
fn timezone_summer_dst_offset() {
    // 09:00 America/New_York in July = EDT (UTC-4) → 13:00 UTC.
    let tz = parse_timezone("America/New_York").expect("known zone");
    let after = Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap();
    let next = next_run_utc("0 9 * * *", tz, after).expect("a next run");
    assert_eq!(next, Utc.with_ymd_and_hms(2026, 7, 1, 13, 0, 0).unwrap());
}

#[test]
fn timezone_winter_dst_offset() {
    // 09:00 America/New_York in January = EST (UTC-5) → 14:00 UTC.
    let tz = parse_timezone("America/New_York").expect("known zone");
    let after = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let next = next_run_utc("0 9 * * *", tz, after).expect("a next run");
    assert_eq!(next, Utc.with_ymd_and_hms(2026, 1, 1, 14, 0, 0).unwrap());
}

#[test]
fn utc_job_is_unshifted() {
    let after = Utc.with_ymd_and_hms(2026, 6, 17, 0, 0, 0).unwrap();
    let next = next_run_utc("30 14 * * *", UTC, after).expect("a next run");
    assert_eq!(next, Utc.with_ymd_and_hms(2026, 6, 17, 14, 30, 0).unwrap());
}

// ── parsing / validation ────────────────────────────────────────────────────

#[test]
fn known_and_unknown_timezones() {
    assert!(parse_timezone("UTC").is_some());
    assert!(parse_timezone("America/New_York").is_some());
    assert!(parse_timezone("Europe/London").is_some());
    assert!(parse_timezone("Mars/Phobos").is_none());
    assert!(parse_timezone("not a zone").is_none());
}

#[test]
fn malformed_expression_yields_nothing() {
    let after = Utc.with_ymd_and_hms(2026, 6, 17, 0, 0, 0).unwrap();
    assert!(upcoming_in_tz("not valid", UTC, 3, after).is_empty());
    assert!(next_run_utc("not valid", UTC, after).is_none());
}

#[test]
fn upcoming_count_is_capped() {
    let after = Utc.with_ymd_and_hms(2026, 6, 17, 0, 0, 0).unwrap();
    assert_eq!(upcoming_in_tz("0 * * * *", UTC, 3, after).len(), 3);
    assert_eq!(upcoming_in_tz("0 * * * *", UTC, 1, after).len(), 1);
}
