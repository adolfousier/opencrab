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

/// #1077: when the file writer mutex is held by a slow/blocked write,
/// `make_writer()` must NOT block indefinitely. It should return a guard
/// that discards writes (via `try_lock()` + `WouldBlock` path).
#[test]
fn make_writer_does_not_block_on_contended_mutex() {
    use crate::logging::ResilientFileWriter;
    use std::io::Write;
    use std::sync::Arc;
    use std::time::Duration;
    use tracing_subscriber::fmt::writer::MakeWriter;

    let dir = std::env::temp_dir().join(format!(
        "opencrabs-log-contention-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let writer = Arc::new(ResilientFileWriter::new(
        dir.clone(),
        "opencrabs".to_string(),
    ));

    // Hold the mutex from another thread to simulate a blocked write.
    let writer2 = Arc::clone(&writer);
    let handle = std::thread::spawn(move || {
        // Acquire the inner mutex and hold it for 500ms.
        let _guard = writer2
            .appender_lock_for_test()
            .expect("must acquire lock");
        std::thread::sleep(Duration::from_millis(500));
    });

    // Give the spawned thread time to acquire the lock.
    std::thread::sleep(Duration::from_millis(50));

    // This call must NOT block for 500ms — it should return immediately
    // with a guard that discards writes.
    let start = std::time::Instant::now();
    let mut w = writer.make_writer();
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_millis(100),
        "make_writer must return quickly under contention, took {elapsed:?}"
    );

    // Writing to the contended guard should succeed (discarded silently).
    let result = w.write_all(b"this should be discarded\n");
    assert!(result.is_ok(), "discarded write must not error");

    handle.join().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}
