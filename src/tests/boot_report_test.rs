//! Boot report ledger (#1242): sessions vs rows, and a wake that never left.

use crate::brain::agent::service::boot_report::*;
use std::sync::Mutex;
use uuid::Uuid;

/// Serialize tests against the process-global ledger, starting each from
/// a clean slate — the same lesson restart_recovery learned the hard way
/// (#1206): a second suite with its own lock does not serialize against
/// the first.
fn guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    let g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset_for_test();
    g
}

fn uuid(n: u8) -> Uuid {
    Uuid::from_u64_pair(n as u64, 0)
}

#[test]
fn an_empty_boot_still_says_so() {
    let _g = guard();
    assert_eq!(
        summary_line(),
        "[boot] interrupted=0 resumed=[] delivered=0 failed=0"
    );
}

#[test]
fn duplicate_rows_count_as_one_session_but_every_resume_counts() {
    let _g = guard();
    // Two rows for session 1 (a re-queued turn), one for session 2.
    record_interrupted(uuid(1));
    record_interrupted(uuid(1));
    record_interrupted(uuid(2));
    record_resumed(uuid(1));
    record_resumed(uuid(2));
    record_delivered();
    record_failed();
    let line = summary_line();
    // Sessions, not rows: the ids are in full precisely so this line is
    // the whole investigation.
    assert!(line.contains("interrupted=2"), "line was: {line}");
    assert!(line.contains(&uuid(1).to_string()), "line was: {line}");
    assert!(line.contains(&uuid(2).to_string()), "line was: {line}");
    assert!(line.contains("delivered=1"), "line was: {line}");
    assert!(line.contains("failed=1"), "line was: {line}");
}

#[test]
fn a_wake_that_never_left_survives_as_failed() {
    let _g = guard();
    record_interrupted(uuid(7));
    record_resumed(uuid(7));
    // The dispatch happened, the transport never came up.
    record_failed();
    let line = summary_line();
    assert!(line.contains("interrupted=1"), "line was: {line}");
    assert!(line.contains(&uuid(7).to_string()), "line was: {line}");
    assert!(line.contains("delivered=0"), "line was: {line}");
    assert!(line.contains("failed=1"), "line was: {line}");
}

#[test]
fn a_dead_tracking_table_is_named_in_the_summary() {
    let _g = guard();
    // Three days of `interrupted=0` hid a table that could not take a row
    // (#1401). The line must say recovery is off and why, even at zero.
    record_tracking_disabled("no such column: origin".to_string());
    let line = summary_line();
    assert!(line.starts_with("[boot] interrupted=0"), "line was: {line}");
    assert!(
        line.ends_with(" recovery=DISABLED(no such column: origin)"),
        "line was: {line}"
    );
}

#[test]
fn a_healthy_boot_does_not_mention_recovery_state() {
    let _g = guard();
    assert!(!summary_line().contains("recovery="));
}
