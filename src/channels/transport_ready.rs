//! Waiting for a channel's transport to finish connecting, instead of
//! dropping the work that needed it (#1242).
//!
//! A wake for a channel session resolves its route, resolves its target, and
//! then asks the channel for the connected client it must send through. At
//! boot that client is often not there yet: the recovery pass and the
//! channel's own connect run concurrently, and nothing orders them. Every
//! adapter answered the same way, with a warn and a `return`:
//!
//! ```text
//! [bg-resume] telegram: bot not available; dropping resume
//! ```
//!
//! That is a permanent loss for a transport that was seconds away. It was
//! observed across ~21 daemon restarts in 48h, and in one case a background
//! result was never delivered at all, because the only thing that would have
//! retried it was the user happening to send a message.
//!
//! Waiting costs a boot a few hundred milliseconds and turns the race into a
//! non-event. The wait is bounded, because a channel that is not configured
//! in this run never connects and holding the task forever would leak one per
//! wake.

use std::future::Future;
use std::time::Duration;
use uuid::Uuid;

/// How long a wake waits for its channel's transport before giving up.
///
/// Deliberately the same window as
/// [`ROUTE_GRACE`](crate::brain::agent::service::restart_recovery::ROUTE_GRACE),
/// which is how long a parked restart report waits for a route before being
/// flushed locally. The two startup flush paths raced because they had
/// different, unstated readiness assumptions; giving them one window by
/// construction is what makes their ordering deterministic rather than
/// incidental. Change one and this fails to compile against the other.
pub(crate) const CONNECT_GRACE: Duration =
    crate::brain::agent::service::restart_recovery::ROUTE_GRACE;

/// Gap between readiness checks. Short enough that a wake follows its
/// connect closely rather than on the next whole second, cheap enough at this
/// interval that polling costs nothing next to a boot.
const POLL: Duration = Duration::from_millis(250);

/// Wait for `lookup` to produce a transport, up to [`CONNECT_GRACE`].
///
/// Returns `None` only when the whole window passed without one, which means
/// the channel is not coming up in this run. `lookup` is re-run per poll
/// rather than awaited once, because "connected" is a mutable slot the
/// channel fills from its own task, not a future that resolves.
///
/// The first check happens before any sleep, so the ordinary case (the
/// channel connected long ago) is exactly as fast as the bare check it
/// replaced.
pub(crate) async fn await_transport<T, F, Fut>(
    channel: &str,
    session_id: Uuid,
    lookup: F,
) -> Option<T>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Option<T>>,
{
    if let Some(ready) = lookup().await {
        return Some(ready);
    }

    // Said once, not per poll: the interesting facts are that a wake had to
    // wait at all and how it ended, not that it is still waiting.
    tracing::info!(
        "[bg-resume] {channel}: transport not connected yet for session {session_id}; \
         waiting up to {CONNECT_GRACE:?}"
    );

    // One deadline over the whole poll loop rather than a comparison inside
    // it. The hand-rolled version measured the bound on `std::time::Instant`
    // while sleeping on tokio's clock: two clocks, agreeing only by luck, and
    // not agreeing at all wherever tokio's is driven independently of the
    // wall (which is exactly how this is tested).
    let started = tokio::time::Instant::now();
    let poll = async {
        loop {
            tokio::time::sleep(POLL).await;
            if let Some(ready) = lookup().await {
                return ready;
            }
        }
    };
    match tokio::time::timeout(CONNECT_GRACE, poll).await {
        Ok(ready) => {
            tracing::info!(
                "[bg-resume] {channel}: transport connected after {:?}; resuming session \
                 {session_id}",
                started.elapsed()
            );
            Some(ready)
        }
        Err(_) => {
            // Error, not warn: this is the drop the issue is about. It is
            // now the exception rather than the boot-time norm, and it means
            // the channel never came up in this run.
            tracing::error!(
                "[bg-resume] {channel}: transport never connected within {CONNECT_GRACE:?}; \
                 session {session_id} was not resumed"
            );
            None
        }
    }
}
