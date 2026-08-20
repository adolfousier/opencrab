//! `flock` outcome classification and EINTR retry.
//!
//! The three lock sites used to read every `flock` failure as "another process
//! holds this", so a signal landing mid-call looked exactly like contention:
//! the scheduler silently declined to start and nothing was logged. These pin
//! the three outcomes apart and prove a signal is retried, not surrendered to.
//!
//! `#![cfg(unix)]`: `flock` and these errnos are unix-only.
#![cfg(unix)]

use std::cell::Cell;

use crate::config::flock::{FlockOutcome, resolve};

#[test]
fn a_clean_lock_is_acquired() {
    let outcome = resolve(|| Ok(()));
    assert!(matches!(outcome, FlockOutcome::Acquired));
}

#[test]
fn ewouldblock_is_the_only_return_that_means_held() {
    let outcome = resolve(|| Err(libc::EWOULDBLOCK));
    assert!(
        matches!(outcome, FlockOutcome::Held),
        "contention must be reported as held so the caller stands down quietly"
    );
}

#[test]
fn a_real_error_is_reported_as_failure_not_as_contention() {
    // Claiming "held" here would send the operator looking for a process that
    // does not exist, and would hide a permission or filesystem problem.
    let outcome = resolve(|| Err(libc::EPERM));
    match outcome {
        FlockOutcome::Failed(e) => assert_eq!(e.raw_os_error(), Some(libc::EPERM)),
        other => panic!("EPERM must not pass for contention, got {other:?}"),
    }
}

#[test]
fn a_signal_is_retried_rather_than_read_as_contention() {
    // The regression this module exists for: one EINTR used to abort the whole
    // request, and the caller treated that as another process holding the lock.
    let calls = Cell::new(0);
    let outcome = resolve(|| {
        calls.set(calls.get() + 1);
        if calls.get() == 1 {
            Err(libc::EINTR)
        } else {
            Ok(())
        }
    });
    assert!(matches!(outcome, FlockOutcome::Acquired));
    assert_eq!(calls.get(), 2, "the interrupted call must be reissued");
}

#[test]
fn repeated_signals_give_up_rather_than_spinning_forever() {
    let calls = Cell::new(0);
    let outcome = resolve(|| {
        calls.set(calls.get() + 1);
        Err(libc::EINTR)
    });
    match outcome {
        FlockOutcome::Failed(e) => assert_eq!(e.raw_os_error(), Some(libc::EINTR)),
        other => panic!("a signal storm must end in failure, got {other:?}"),
    }
    assert!(
        calls.get() > 1 && calls.get() < 100,
        "retries must be bounded, saw {}",
        calls.get()
    );
}
