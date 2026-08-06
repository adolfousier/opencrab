//! Provider health registry — quota-exhaustion circuit breaker (#952).
//!
//! A provider whose API reports a HARD quota / billing limit (monthly cap,
//! free tier exhausted, no credit) is marked exhausted for a TTL window.
//! While marked:
//!
//! - fallback walks in the tool loop skip it entirely (no dead requests),
//! - a later successful response clears the mark instantly — a 200 proves
//!   genuine recovery (a hard-quota-dead provider never fluke-succeeds),
//!   so no TTL wait is needed to heal.
//!
//! The registry is process-global: quota is per provider account/key and
//! shared by every session. One probe request per turn still reaches the
//! provider (fast-fail, zero backoff — `is_retryable` returns false for
//! quota errors), so genuine recovery is detected on the very next turn
//! even before the TTL expires.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Default window a provider stays marked quota-exhausted. Long enough to
/// stop the per-turn retry storm, short enough to heal within a session
/// when the quota window genuinely resets.
pub const QUOTA_EXHAUSTION_TTL: Duration = Duration::from_secs(60 * 60);

fn registry() -> &'static Mutex<HashMap<String, Instant>> {
    static REG: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(HashMap::new()))
}

fn key(provider: &str) -> String {
    provider.to_lowercase()
}

/// Mark a provider quota-exhausted for the default TTL.
pub fn mark_exhausted(provider: &str) {
    mark_exhausted_for(provider, QUOTA_EXHAUSTION_TTL);
}

/// Mark a provider quota-exhausted for a custom TTL (tests use short ones).
pub fn mark_exhausted_for(provider: &str, ttl: Duration) {
    if provider.is_empty() {
        return;
    }
    if let Ok(mut map) = registry().lock() {
        map.insert(key(provider), Instant::now() + ttl);
        tracing::warn!(
            "Provider '{}' marked quota-exhausted for {}s (#952)",
            provider,
            ttl.as_secs()
        );
    }
}

/// True while the provider is inside its exhaustion window. Expired
/// entries are pruned on read.
pub fn is_exhausted(provider: &str) -> bool {
    let Ok(mut map) = registry().lock() else {
        return false;
    };
    let k = key(provider);
    match map.get(&k) {
        Some(until) if *until > Instant::now() => true,
        Some(_) => {
            map.remove(&k);
            false
        }
        None => false,
    }
}

/// Clear one provider (e.g. after the user rotates keys via /onboard).
pub fn clear(provider: &str) {
    if let Ok(mut map) = registry().lock() {
        map.remove(&key(provider));
    }
}

/// Clear every entry — test isolation.
pub fn clear_all() {
    if let Ok(mut map) = registry().lock() {
        map.clear();
    }
}

/// Names of all currently-exhausted providers (user-facing summaries).
pub fn exhausted_snapshot() -> Vec<String> {
    let Ok(mut map) = registry().lock() else {
        return Vec::new();
    };
    let now = Instant::now();
    map.retain(|_, until| *until > now);
    let mut names: Vec<String> = map.keys().cloned().collect();
    names.sort();
    names
}
