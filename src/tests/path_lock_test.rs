//! One writer at a time, per path.
//!
//! Agents sharing a working tree wrote the same bytes with no arbitration.
//! Sub-agents have their own checkout now (#1151), but that degrades to the
//! parent's directory outside a repository or when git refuses, and the
//! collision returns with it.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use tempfile::TempDir;

use crate::brain::tools::path_lock::{acquire, contention_notice};

#[test]
fn an_uncontended_write_gets_the_lock() {
    // The common case: one agent editing its own file must pay nothing and
    // see nothing.
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("only-writer.txt");
    std::fs::write(&target, "x").unwrap();

    let lock = acquire(&target).expect("a lock is always returned");
    assert!(lock.is_held(), "nobody else is writing");
}

#[test]
fn a_second_writer_is_told_it_did_not_get_the_lock() {
    // Not an error: the write still happens. What matters is that the second
    // writer knows it overlapped, so the overlap can be reported instead of
    // silently producing a file neither agent intended.
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("contended.txt");
    std::fs::write(&target, "x").unwrap();

    let first = acquire(&target).expect("first lock");
    assert!(first.is_held());

    let second = acquire(&target).expect("second lock is still returned");
    assert!(
        !second.is_held(),
        "the second writer must know it did not hold the lock"
    );
}

#[test]
fn releasing_lets_the_next_writer_in() {
    // A holder that finishes must not leave the path wedged for everyone else.
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("sequential.txt");
    std::fs::write(&target, "x").unwrap();

    {
        let first = acquire(&target).expect("first lock");
        assert!(first.is_held());
    }

    let second = acquire(&target).expect("second lock");
    assert!(second.is_held(), "the lock was released with the guard");
}

#[test]
fn a_holder_that_panics_still_releases() {
    // A crashed writer must cost a pause, not a permanently unwritable file.
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("panicky.txt");
    std::fs::write(&target, "x").unwrap();
    let path = target.clone();

    let _ = std::panic::catch_unwind(move || {
        let _lock = acquire(&path).expect("lock");
        panic!("writer died mid-write");
    });

    let after = acquire(&target).expect("lock");
    assert!(after.is_held(), "the dead writer's lock is gone");
}

#[test]
fn separate_paths_never_contend() {
    // Locking is per file. Two agents working on different files are the
    // normal case and must run fully in parallel.
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("a.txt");
    let b = dir.path().join("b.txt");
    std::fs::write(&a, "a").unwrap();
    std::fs::write(&b, "b").unwrap();

    let lock_a = acquire(&a).expect("lock a");
    let lock_b = acquire(&b).expect("lock b");

    assert!(lock_a.is_held() && lock_b.is_held());
}

#[test]
fn concurrent_writers_take_the_lock_one_at_a_time() {
    // The property the whole thing exists for, exercised with real threads
    // rather than sequential calls: however many writers arrive, only one
    // holds the path at any moment.
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("shared.txt");
    std::fs::write(&target, "x").unwrap();

    let inside = Arc::new(AtomicUsize::new(0));
    let overlaps = Arc::new(AtomicUsize::new(0));

    let mut handles = Vec::new();
    for _ in 0..4 {
        let target = target.clone();
        let inside = Arc::clone(&inside);
        let overlaps = Arc::clone(&overlaps);
        handles.push(thread::spawn(move || {
            let lock = acquire(&target).expect("lock");
            if lock.is_held() {
                if inside.fetch_add(1, Ordering::SeqCst) != 0 {
                    overlaps.fetch_add(1, Ordering::SeqCst);
                }
                thread::sleep(Duration::from_millis(20));
                inside.fetch_sub(1, Ordering::SeqCst);
            }
        }));
    }
    for h in handles {
        h.join().expect("thread");
    }

    assert_eq!(
        overlaps.load(Ordering::SeqCst),
        0,
        "two holders were inside the lock at once"
    );
}

#[test]
fn the_notice_names_the_file_and_what_it_means() {
    // The loser has to be able to act on this, which means knowing which file
    // and that its contents are now suspect.
    let notice = contention_notice(std::path::Path::new("/tmp/thing.rs"));
    assert!(notice.contains("/tmp/thing.rs"));
    assert!(notice.contains("re-read"), "got: {notice}");
}
