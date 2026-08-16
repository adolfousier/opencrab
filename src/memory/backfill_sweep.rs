//! Keep embedding a memory store that a failed backfill left at zero (#1069).
//!
//! Embedding ran in exactly two places: once at startup, inside `reindex`, and
//! on the write path for the one document being written. Neither retries. So a
//! backfill that failed for a transient reason (a bad key, an endpoint that was
//! down, a rate limit) left every document unembedded until someone restarted
//! the process, and nothing said so. Search degraded to keyword-only FTS and
//! kept answering, which is why an install ran 94 days with one vectorised
//! chunk out of 589 and nobody noticed.
//!
//! This is the retry under that. A timer, not a queue: the store already knows
//! what needs embedding (`get_hashes_needing_embedding`), so there is no state
//! to keep and nothing to lose across a restart.
//!
//! ## Why config is re-read every tick
//!
//! `read_memory_config` parses config.toml on each call, so a key fixed
//! mid-session is picked up on the next tick with no cache to invalidate and no
//! reload plumbing. That is also why the interval is resolved per tick rather
//! than captured once: changing it in config.toml takes effect within one
//! period instead of at the next restart.
//!
//! ## Single flight
//!
//! A tick that lands while the previous one is still embedding skips instead of
//! stacking. Non-negotiable on the local path: `freshness.rs` documents that
//! llama-cpp GGML can segfault under contention, and a sweep is precisely a
//! second embedder entering alongside the first.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::db::Store;

/// Set while a sweep is embedding, so the next tick skips rather than running
/// a second embedder alongside it.
static SWEEPING: AtomicBool = AtomicBool::new(false);

/// The sweep period for a given config, or `None` when the sweep is off.
///
/// Off means either an explicit `backfill_interval_secs = 0` or
/// `vector_enabled = false`. The second case matters: #1062 established that
/// disabling vectors skips embedding work entirely, local and API alike, and a
/// timer that wakes every five minutes to decide it has nothing to do is still
/// work the user asked not to happen.
///
/// Takes the config rather than reading it so the decision can be tested
/// without a config.toml on disk.
pub(crate) fn interval_for(cfg: &crate::config::MemoryConfig) -> Option<Duration> {
    if !cfg.vector_enabled || cfg.backfill_interval_secs == 0 {
        return None;
    }
    Some(Duration::from_secs(cfg.backfill_interval_secs))
}

/// The configured sweep period for this install.
fn sweep_interval() -> Option<Duration> {
    interval_for(&super::read_memory_config())
}

/// Start the periodic backfill sweep for this process.
///
/// Call once at boot, after the startup reindex, so the first tick can never
/// race the backfill that reindex already runs.
pub fn spawn(store: &'static Mutex<Store>) {
    tokio::spawn(async move {
        loop {
            // Resolved per iteration rather than once outside the loop: an
            // interval edited in config.toml then takes effect within one
            // period, and turning the sweep off stops it without a restart.
            let Some(period) = sweep_interval() else {
                tracing::debug!("Memory backfill sweep: disabled by config, stopping");
                return;
            };
            tokio::time::sleep(period).await;
            run_once(store).await;
        }
    });
}

/// One sweep pass. Public to the crate so the interval logic and the skip
/// behaviour can be exercised without waiting on a timer.
pub(crate) async fn run_once(store: &'static Mutex<Store>) {
    if !try_enter() {
        tracing::debug!("Memory backfill sweep: already in flight, skipping");
        return;
    }

    sweep_inner(store).await;
    leave();
}

/// Claim the single-flight slot. `false` means another sweep holds it and this
/// tick must skip rather than embed alongside it.
///
/// Split out from [`run_once`] so the skip can be tested without a store: the
/// alternative is a test that actually embeds, which needs a model or a live
/// endpoint and would prove nothing about the guard.
pub(crate) fn try_enter() -> bool {
    SWEEPING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}

/// Release the single-flight slot.
pub(crate) fn leave() {
    SWEEPING.store(false, Ordering::Release);
}

async fn sweep_inner(store: &'static Mutex<Store>) {
    if super::embedding_api_configured() {
        let (stored, needing) = super::embedding::run_api_backfill(store).await;
        // Logged only when there was work. A five-minute timer that reports
        // "0 documents need embeddings" forever is noise that trains people to
        // stop reading the log, which is the same blindness this fixes.
        if needing > 0 {
            tracing::info!("Memory backfill sweep: embedded {stored}/{needing} documents");
        }
        return;
    }

    if !super::vector_enabled() {
        return;
    }

    // Local path. `backfill_embeddings` is blocking and enters llama.cpp, so it
    // goes to a blocking thread like every other caller of it.
    if let Err(e) =
        tokio::task::spawn_blocking(move || super::embedding::backfill_embeddings(store)).await
    {
        tracing::warn!("Memory backfill sweep: local backfill task failed: {e}");
    }
}
