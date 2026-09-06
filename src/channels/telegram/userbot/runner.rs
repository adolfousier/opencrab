//! Lifetime binding for the MTProto connection driver.
//!
//! `SenderPool::new` hands back a runner future that owns every socket to
//! Telegram. Spawning it and dropping the `JoinHandle` (the shape the
//! original PR had) meant the manager could only abort the update loop: the
//! runner kept the auth key and its sockets alive after `enabled = false`,
//! and a reconcile restart opened a second pool on the same session file
//! with two writers racing `save_if_dirty`. The guard ties the runner's life
//! to whoever holds it: dropping the guard aborts the task, so an aborted
//! watch loop takes its connection down with it and the login CLI releases
//! the pool when it returns.

use tokio::task::JoinHandle;

/// Aborts the wrapped task when dropped.
pub(crate) struct AbortOnDrop(JoinHandle<()>);

impl AbortOnDrop {
    pub(crate) fn new(handle: JoinHandle<()>) -> Self {
        Self(handle)
    }
}

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}
