//! Regression for issue #190 (secondary): the rolling daily log files are named
//! `opencrabs.YYYY-MM-DD` with NO `.log` extension. The old
//! `path.extension() == "log"` checks therefore matched ZERO real log files, so
//! `logs status` reported `Log files: 0`, `logs view` found nothing, and
//! `cleanup_old_logs` never pruned anything. `is_log_file` matches the real
//! naming instead.

use crate::logging::is_log_file;

#[test]
fn matches_rolling_daily_log_files() {
    // The exact filename from the #190 report.
    assert!(is_log_file("opencrabs.2026-06-10"));
    assert!(is_log_file("opencrabs.2026-06-11"));
}

#[test]
fn rejects_unrelated_files() {
    assert!(!is_log_file(".gitignore"));
    assert!(
        !is_log_file("opencrabs"),
        "a bare prefix with no date suffix is not a rolling log file"
    );
    assert!(
        !is_log_file("other.2026-06-10"),
        "a different prefix must not match"
    );
    assert!(!is_log_file("nginx.log"));
}

#[test]
fn old_extension_check_would_have_missed_the_real_files() {
    // Document the root cause: `Path::extension()` on `opencrabs.2026-06-10`
    // returns the DATE (everything after the last dot), never "log" — so the
    // previous `path.extension().map(|e| e == "log")` was false for every file.
    let real = "opencrabs.2026-06-10";
    assert_ne!(
        std::path::Path::new(real)
            .extension()
            .and_then(|e| e.to_str()),
        Some("log"),
        "the rolling file's extension is the date, not \"log\" — that's why the old check matched nothing"
    );
    assert!(
        is_log_file(real),
        "the new matcher catches what the old missed"
    );
}
