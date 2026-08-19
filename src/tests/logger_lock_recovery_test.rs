//! The log writer survives a poisoned mutex and never drops events (#1077, #1115).
//!
//! #1077: a panic while holding the appender lock poisoned it, and every later
//! write failed, silencing logging for the rest of the run.
//!
//! #1115: the fix for that also swapped `lock()` for `try_lock()` and discarded
//! the event on `WouldBlock`. `WouldBlock` only means another thread holds the
//! lock right now, which under normal concurrent logging is constant, so log
//! lines were thrown away under ordinary load. It also announced each drop with
//! `eprintln!`, which corrupts a TUI that owns the terminal.

use crate::logging::logger::ResilientFileWriter;
use std::io::Write;
use std::sync::Arc;
use tracing_subscriber::fmt::writer::MakeWriter;

fn writer_in(dir: &std::path::Path) -> ResilientFileWriter {
    ResilientFileWriter::new(dir.to_path_buf(), "test".to_string())
}

#[test]
fn a_write_lands_in_the_log_file() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let w = writer_in(tmp.path());

    w.make_writer().write_all(b"hello\n").expect("write");

    let wrote_something = std::fs::read_dir(tmp.path())
        .expect("read dir")
        .filter_map(Result::ok)
        .any(|e| e.metadata().map(|m| m.len() > 0).unwrap_or(false));
    assert!(wrote_something, "the event reached a file");
}

#[test]
fn a_poisoned_mutex_still_yields_a_usable_writer() {
    // #1077: recovery via into_inner. A panic that poisons the lock must not
    // silence logging for the rest of the process.
    let tmp = tempfile::tempdir().expect("tempdir");
    let w = Arc::new(writer_in(tmp.path()));

    let poisoner = Arc::clone(&w);
    let _ = std::thread::spawn(move || {
        let _guard = poisoner.make_writer();
        panic!("poison the appender lock");
    })
    .join();

    // The lock is now poisoned; the writer must still work.
    w.make_writer()
        .write_all(b"after poisoning\n")
        .expect("write must still succeed on a poisoned lock");
}

#[test]
fn concurrent_writers_all_get_through() {
    // #1115: try_lock dropped an event whenever another thread held the lock,
    // which is the normal case under concurrency. Blocking keeps every event.
    let tmp = tempfile::tempdir().expect("tempdir");
    let w = Arc::new(writer_in(tmp.path()));

    let handles: Vec<_> = (0..8)
        .map(|i| {
            let w = Arc::clone(&w);
            std::thread::spawn(move || {
                for n in 0..25 {
                    w.make_writer()
                        .write_all(format!("thread {i} line {n}\n").as_bytes())
                        .expect("write");
                }
            })
        })
        .collect();
    for h in handles {
        h.join().expect("thread");
    }

    let total: u64 = std::fs::read_dir(tmp.path())
        .expect("read dir")
        .filter_map(Result::ok)
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum();
    // 200 writes of ~15 bytes. An exact count is brittle across rotation, but
    // a try_lock that drops under contention would leave this far short.
    assert!(
        total > 1500,
        "expected every concurrent event to be written, got {total} bytes"
    );
}
