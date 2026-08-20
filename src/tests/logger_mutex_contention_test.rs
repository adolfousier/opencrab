//! Appender-mutex behaviour under contention (#1077, #1115).
//!
//! #1077: a panic while holding the appender lock poisoned it and silenced
//! logging for the rest of the run. Recovery via `into_inner` fixes that.
//!
//! #1115: the same change also swapped `lock()` for `try_lock()` and discarded
//! the event on `WouldBlock`. `WouldBlock` only means another thread holds the
//! lock right now, which under concurrent logging is routine, so log lines were
//! thrown away under ordinary load — and each drop was announced with
//! `eprintln!`, which corrupts a TUI that owns the terminal.
//!
//! The contract is now: wait for the lock, then write. This file asserts that,
//! where it previously asserted the opposite.

use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tracing_subscriber::fmt::MakeWriter;

#[test]
fn a_contended_writer_waits_rather_than_discarding_the_event() {
    use crate::logging::logger::ResilientFileWriter;

    let writer = Arc::new(ResilientFileWriter::new_for_test());

    // Hold the lock, then release it while a second thread is waiting.
    let guard = writer.appender_lock_for_test();

    let writer_clone = Arc::clone(&writer);
    let handle = thread::spawn(move || {
        let start = std::time::Instant::now();
        let _w = writer_clone.make_writer();
        // It waited for the holder instead of skipping the event. This test
        // previously asserted the reverse — that make_writer returns inside
        // 100ms with a guard that throws the write away — which was the
        // defect, not the requirement (#1115).
        assert!(
            start.elapsed() >= Duration::from_millis(100),
            "make_writer must wait for the lock rather than drop the event"
        );
    });

    // Give the waiter time to block on the lock, then let it through.
    thread::sleep(Duration::from_millis(150));
    drop(guard);

    handle.join().expect("thread panicked");
}

#[test]
fn uncontended_lock_succeeds() {
    use crate::logging::logger::ResilientFileWriter;

    let writer = ResilientFileWriter::new_for_test();

    // Without contention, make_writer should succeed quickly
    let start = std::time::Instant::now();
    let _w = writer.make_writer();
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_millis(50),
        "uncontended make_writer() took {:?}, expected < 50ms",
        elapsed
    );
}

#[test]
fn multiple_threads_do_not_deadlock() {
    use crate::logging::logger::ResilientFileWriter;

    let writer = Arc::new(ResilientFileWriter::new_for_test());

    // Spawn multiple threads that all try to make writers concurrently
    let handles: Vec<_> = (0..10)
        .map(|_| {
            let writer_clone = Arc::clone(&writer);
            thread::spawn(move || {
                for _ in 0..100 {
                    let _w = writer_clone.make_writer();
                    thread::yield_now();
                }
            })
        })
        .collect();

    // All threads should complete without deadlocking
    for handle in handles {
        handle.join().expect("thread panicked");
    }
}
