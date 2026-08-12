//! The built-in dedup-scan job must actually schedule (#1024).
//!
//! It shipped as `0 4 * * 0` — Sunday in Unix numbering — which the `cron`
//! crate rejects outright, so the job never ran once and the scheduler logged
//! a parse failure every minute for its whole lifetime. The cross-file dedup
//! scan is what catches the same rule being written into two brain files, so
//! its silence is why a live workspace accumulated 63 near-duplicate rules and
//! a permissions rule that drifted into two contradictory statements.
//!
//! The trap this pins: the obvious fix is to translate Unix `0` to `7`, and
//! that is WRONG. This crate numbers 1 = Sunday, so 7 is Saturday and the job
//! would run on the wrong day, silently — worse than not running at all.

use crate::cron::next_run_utc;
use crate::cron::scheduler::DEDUP_SCAN_CRON;
use chrono::{Datelike, TimeZone, Utc, Weekday};
use chrono_tz::UTC;

/// The shipped expression must parse, or the job silently never runs.
#[test]
fn the_dedup_scan_expression_parses() {
    assert!(
        next_run_utc(DEDUP_SCAN_CRON, UTC, Utc::now()).is_some(),
        "'{}' does not parse — the job would log a failure every tick and \
         never run, which is exactly the state this fixes",
        DEDUP_SCAN_CRON
    );
}

/// And it must land on Sunday, not merely parse.
///
/// Translating Unix 0 to 7 also parses, and puts the job on Saturday.
#[test]
fn the_dedup_scan_runs_on_sunday() {
    let after = Utc.with_ymd_and_hms(2026, 6, 17, 12, 0, 0).unwrap();
    let next = next_run_utc(DEDUP_SCAN_CRON, UTC, after).expect("must schedule");
    assert_eq!(
        next.weekday(),
        Weekday::Sun,
        "'{}' fires on {:?}; 1 = Sunday in this crate, 7 would be Saturday",
        DEDUP_SCAN_CRON,
        next.weekday()
    );
}

/// Unix-style Sunday must still be rejected, not silently translated.
#[test]
fn a_unix_style_sunday_is_still_rejected() {
    assert!(
        next_run_utc("0 4 * * 0", UTC, Utc::now()).is_none(),
        "translating 0 to 7 would schedule Saturday; the error is the correct \
         outcome and is surfaced to whoever wrote the expression"
    );
}
