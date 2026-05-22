//! Per-provider token calibration.
//!
//! `brain::tokenizer::count_tokens` uses cl100k_base (the OpenAI/Anthropic
//! tokenizer). Many providers — notably Qwen-family models — tokenize the
//! same text very differently. For `qwen-latest-series-invite-beta-v34`,
//! a cl100k_base estimate of `system_brain + tool_schemas + messages` of
//! ~24k tokens turns into ~10k real input tokens reported by the provider.
//! That mismatch is what produces the "footer started at 24k then dropped
//! to 10k after the first hi" UX confusion.
//!
//! Strategy: observe `real_input_tokens / local_estimate` per provider on
//! each successful API turn, store as a single float per provider in
//! `~/.opencrabs/token_calibration.json`, and apply that ratio when we
//! compute a local estimate for display. The first turn of a session may
//! still be off (no observations yet, ratio defaults to 1.0), but every
//! subsequent session for that provider lands close to reality from the
//! very first frame.
//!
//! Scope: only affects *display* estimates (the ctx footer's initial
//! value before any API response has arrived). Budget logic (compaction
//! trigger, context limit enforcement) keeps the raw cl100k_base count as
//! a safe overestimate — better to compact a hair early than to send an
//! over-budget prompt.

use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// In-memory cache of provider → ratio, hydrated from disk on first access.
static CALIBRATION: Lazy<Mutex<HashMap<String, f64>>> =
    Lazy::new(|| Mutex::new(load_from_disk()));

/// EMA blend factor for new observations. Low value (= slow updates) so a
/// single weird turn (e.g. an unusually large tool result) doesn't yank
/// the ratio around. Five clean turns get us within ~70% of the new value.
const EMA_ALPHA: f64 = 0.3;

/// Reject observations where either side is too small — the noise floor of
/// the cl100k_base estimator + small-message tokenization quirks dominates
/// below this threshold and produces unstable ratios.
const MIN_OBSERVATION_TOKENS: u32 = 500;

/// Clamp observed ratios to a plausible band — outside this range almost
/// certainly indicates a bug (wrong denominator, provider returning 0,
/// double-counted tool schemas, etc.) and would poison the cache.
const RATIO_MIN: f64 = 0.10;
const RATIO_MAX: f64 = 5.00;

fn calibration_path() -> PathBuf {
    crate::config::opencrabs_home().join("token_calibration.json")
}

fn load_from_disk() -> HashMap<String, f64> {
    let path = calibration_path();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return HashMap::new();
    };
    match serde_json::from_str::<HashMap<String, f64>>(&text) {
        Ok(map) => {
            // Defensive: drop any entries outside the plausible range so a
            // bad earlier write can't keep skewing the display forever.
            map.into_iter()
                .filter(|(_, r)| (RATIO_MIN..=RATIO_MAX).contains(r))
                .collect()
        }
        Err(e) => {
            tracing::warn!(
                "token_calibration: failed to parse {}: {} — starting fresh",
                path.display(),
                e
            );
            HashMap::new()
        }
    }
}

fn save_to_disk(map: &HashMap<String, f64>) {
    let path = calibration_path();
    let Ok(json) = serde_json::to_string_pretty(map) else {
        return;
    };
    if let Err(e) = std::fs::write(&path, json) {
        tracing::warn!(
            "token_calibration: failed to persist to {}: {}",
            path.display(),
            e
        );
    }
}

/// Returns the calibration ratio for `provider`, or `None` if we have not
/// observed enough turns yet to be confident. Callers should fall back to
/// the raw local estimate when this returns `None`.
pub fn get_ratio(provider: &str) -> Option<f64> {
    CALIBRATION.lock().ok()?.get(provider).copied()
}

/// Apply the calibration ratio (if any) to a raw cl100k_base estimate.
/// Returns the raw estimate unchanged when no ratio is known yet.
pub fn calibrate(provider: &str, raw_estimate: u32) -> u32 {
    match get_ratio(provider) {
        Some(ratio) => ((raw_estimate as f64) * ratio).round() as u32,
        None => raw_estimate,
    }
}

/// Record one (local_estimate, real_input_tokens) observation and update
/// the EMA. No-op when either side is below `MIN_OBSERVATION_TOKENS` or the
/// implied ratio is outside `[RATIO_MIN, RATIO_MAX]`.
pub fn record_observation(provider: &str, local_estimate: u32, real_input_tokens: u32) {
    if local_estimate < MIN_OBSERVATION_TOKENS || real_input_tokens < MIN_OBSERVATION_TOKENS {
        return;
    }
    let observed = real_input_tokens as f64 / local_estimate as f64;
    if !(RATIO_MIN..=RATIO_MAX).contains(&observed) {
        tracing::debug!(
            "token_calibration: ignoring out-of-band observation for '{}' \
             (local={}, real={}, ratio={:.3})",
            provider,
            local_estimate,
            real_input_tokens,
            observed
        );
        return;
    }

    let mut map = match CALIBRATION.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let new_ratio = match map.get(provider).copied() {
        None => observed,
        Some(prev) => prev * (1.0 - EMA_ALPHA) + observed * EMA_ALPHA,
    };
    let new_ratio = new_ratio.clamp(RATIO_MIN, RATIO_MAX);
    map.insert(provider.to_string(), new_ratio);
    tracing::debug!(
        "token_calibration: '{}' ratio updated to {:.3} (observed={:.3}, local={}, real={})",
        provider,
        new_ratio,
        observed,
        local_estimate,
        real_input_tokens
    );
    save_to_disk(&map);
}
