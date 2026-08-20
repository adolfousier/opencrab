//! `flock(2)` with the retry POSIX requires, and an outcome callers can act on.
//!
//! A bare `flock` reports contention and genuine failure through the same `-1`,
//! and every caller here collapsed the two: any error meant "another process
//! holds this". A signal arriving mid-call therefore read as contention, so the
//! daemon quietly declined to start its scheduler and cron went unpolled with
//! nothing logged. `EINTR` is transient and must be retried; `EWOULDBLOCK` is
//! the only return that actually means held.

use std::io;
use std::os::unix::io::RawFd;

/// How an exclusive lock request ended.
#[derive(Debug)]
pub(crate) enum FlockOutcome {
    /// The caller now holds the lock, until the file descriptor closes.
    Acquired,
    /// Another live process holds it. Only `EWOULDBLOCK` produces this.
    Held,
    /// The request failed for a reason that is not contention. Never treat
    /// this as held: it means we do not know, and it deserves a log line.
    Failed(io::Error),
}

/// How many times a signal may interrupt one request before we stop retrying.
/// Bounded so a pathological signal storm cannot spin here forever.
const MAX_EINTR_RETRIES: u32 = 8;

/// Drive one lock request to a conclusion, retrying while the kernel reports
/// `EINTR`. `call` reports success as `Ok(())` and failure as the raw errno,
/// which keeps this function pure and testable without touching a real fd.
pub(crate) fn resolve<F>(mut call: F) -> FlockOutcome
where
    F: FnMut() -> Result<(), i32>,
{
    for _ in 0..=MAX_EINTR_RETRIES {
        match call() {
            Ok(()) => return FlockOutcome::Acquired,
            Err(errno) if errno == libc::EINTR => continue,
            Err(errno) if errno == libc::EWOULDBLOCK => return FlockOutcome::Held,
            Err(errno) => return FlockOutcome::Failed(io::Error::from_raw_os_error(errno)),
        }
    }
    FlockOutcome::Failed(io::Error::from_raw_os_error(libc::EINTR))
}

/// Request an exclusive lock on `fd`, adding `LOCK_NB` when the caller must not
/// block. The descriptor must outlive the call.
pub(crate) fn exclusive(fd: RawFd, non_blocking: bool) -> FlockOutcome {
    let op = if non_blocking {
        libc::LOCK_EX | libc::LOCK_NB
    } else {
        libc::LOCK_EX
    };
    resolve(|| {
        // SAFETY: `fd` is borrowed from a File the caller keeps alive across
        // this call, and `op` is a valid flock operation.
        if unsafe { libc::flock(fd, op) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error()
                .raw_os_error()
                .unwrap_or(libc::EIO))
        }
    })
}
