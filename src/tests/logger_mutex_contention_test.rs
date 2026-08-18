/// Regression test for #1077: when the file appender mutex is held by one
/// thread (e.g., a blocking write), other threads must not block indefinitely
/// on `make_writer()`. The `try_lock()` with contention handling ensures
/// that a contended lock returns quickly and the event is discarded rather
/// than blocking the caller.
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tracing_subscriber::fmt::MakeWriter;

#[test]
fn try_lock_contention_returns_quickly() {
    use crate::logging::logger::ResilientFileWriter;

    let writer = Arc::new(ResilientFileWriter::new_for_test());

    // Acquire the lock on the appender to simulate contention
    let _guard = writer.appender_lock_for_test();

    // Spawn a thread that tries to make a writer while the lock is held
    let writer_clone = Arc::clone(&writer);
    let handle = thread::spawn(move || {
        let start = std::time::Instant::now();
        let _w = writer_clone.make_writer();
        let elapsed = start.elapsed();

        // The try_lock should return quickly (within 100ms), not block indefinitely
        assert!(
            elapsed < Duration::from_millis(100),
            "make_writer() blocked for {:?}, expected < 100ms",
            elapsed
        );
    });

    // Wait for the thread to complete
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
