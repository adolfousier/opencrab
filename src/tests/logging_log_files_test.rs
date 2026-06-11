//! Regression for issue #190 (secondary): the rolling daily log files are named
//! `opencrabs.YYYY-MM-DD` with NO `.log` extension. The old
//! `path.extension() == "log"` checks therefore matched ZERO real log files, so
//! `logs status` reported `Log files: 0`, `logs view` found nothing, and
//! `cleanup_old_logs` never pruned anything. `is_log_file` matches the real
//! naming instead.

use crate::logging::is_log_file;

/// The synchronous self-healing writer (#190 primary) must actually create a
/// `opencrabs.<date>` file and write events into it — proving the wiring that
/// replaced the silently-dying `non_blocking` worker. The file it produces must
/// also be one that `is_log_file` recognizes, so the readers can see it.
#[test]
fn resilient_writer_creates_recognized_dated_file_with_content() {
    use crate::logging::ResilientFileWriter;
    use std::io::Write;
    use tracing_subscriber::fmt::writer::MakeWriter;

    let dir = std::env::temp_dir().join(format!("opencrabs-log-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let writer = ResilientFileWriter::new(dir.clone(), "opencrabs".to_string());
    {
        let mut w = writer.make_writer();
        w.write_all(b"hello from the resilient writer\n").unwrap();
        w.flush().unwrap();
    }

    let log_name = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .find(|n| is_log_file(n))
        .expect("writer must create a log file the readers can recognize");

    let content = std::fs::read_to_string(dir.join(&log_name)).unwrap();
    assert!(
        content.contains("hello from the resilient writer"),
        "the event must be written to the file; got: {content:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

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
