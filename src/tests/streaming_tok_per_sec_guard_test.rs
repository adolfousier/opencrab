//! Tests for `tool_loop::compute_streaming_tok_per_sec` — the guard that
//! keeps burst-delivery artifacts out of the channel ctx/tok-s footer.
//!
//! Regression (2026-06-06): a glm-5.1 short reply on Telegram rendered
//! `ctx: 86K/200K 43% | 37203 tok/s`. The provider burst-delivered the
//! whole ~300-token response in a single ~8ms SSE chunk, so the active-
//! streaming window was near-zero and `output / active` exploded. The
//! pre-fix code did `Some(total_output as f64 / total_active)` whenever
//! `total_active > 0.0`, with no floor or ceiling — so any sub-second
//! burst produced a physically impossible rate.
//!
//! The guard returns `None` (footer shows no tok/s) when the measurement
//! isn't credible: zero output, a window below the floor, or a rate above
//! the plausible ceiling.

use crate::brain::agent::service::tool_loop::compute_streaming_tok_per_sec;

#[test]
fn normal_streaming_rate_is_reported() {
    // 900 tokens over 2.0s = 450 tok/s — a believable sustained rate
    // (matches the 447 tok/s the same session showed on a longer reply).
    let rate = compute_streaming_tok_per_sec(900, 2.0).expect("credible rate must be Some");
    assert!((rate - 450.0).abs() < 0.001, "got {rate}");
}

#[test]
fn burst_delivery_near_zero_window_is_suppressed() {
    // The exact regression: ~300 tokens delivered in ~8ms. Pre-fix this
    // produced 37500 tok/s. The floor must reject the sub-window entirely.
    assert_eq!(
        compute_streaming_tok_per_sec(300, 0.008),
        None,
        "an 8ms active window is too coarse to be a rate — must suppress"
    );
}

#[test]
fn window_just_below_floor_is_suppressed() {
    // 0.29s is below the 0.3s floor.
    assert_eq!(compute_streaming_tok_per_sec(100, 0.29), None);
}

#[test]
fn window_at_floor_with_plausible_rate_is_reported() {
    // 0.3s window, 150 tokens → 500 tok/s. At the floor and under the
    // ceiling, so it's reported.
    let rate = compute_streaming_tok_per_sec(150, 0.3).expect("at-floor credible rate");
    assert!((rate - 500.0).abs() < 0.001, "got {rate}");
}

#[test]
fn rate_above_ceiling_is_suppressed_even_when_window_clears_floor() {
    // A multi-burst turn can clear the time floor but still produce an
    // implausible rate: 5000 tokens over 0.5s = 10000 tok/s. No real
    // model streams that fast to an end user — suppress as artifact.
    assert_eq!(
        compute_streaming_tok_per_sec(5000, 0.5),
        None,
        "10000 tok/s is a measurement artifact, not a generation rate"
    );
}

#[test]
fn rate_just_under_ceiling_is_reported() {
    // 1900 tokens over 1.0s = 1900 tok/s — fast specialized inference
    // (Groq/Cerebras territory), still under the 2000 ceiling.
    let rate = compute_streaming_tok_per_sec(1900, 1.0).expect("under-ceiling rate");
    assert!((rate - 1900.0).abs() < 0.001, "got {rate}");
}

#[test]
fn rate_exactly_at_ceiling_is_reported() {
    // 2000 tokens over 1.0s = exactly 2000 tok/s — the boundary is
    // inclusive (`<=`), so it's reported.
    let rate = compute_streaming_tok_per_sec(2000, 1.0).expect("at-ceiling rate");
    assert!((rate - 2000.0).abs() < 0.001, "got {rate}");
}

#[test]
fn zero_output_tokens_is_none() {
    // No tokens generated (e.g. a pure tool-call iteration) — no rate.
    assert_eq!(compute_streaming_tok_per_sec(0, 5.0), None);
}

#[test]
fn zero_active_secs_is_none() {
    // Non-streaming provider / never delivered a delta — no rate, no
    // division by zero.
    assert_eq!(compute_streaming_tok_per_sec(500, 0.0), None);
}

#[test]
fn long_slow_stream_reports_low_rate() {
    // 40 tokens over 10s = 4 tok/s — a slow local model. Credible and
    // must be shown (the floor is a window floor, not a rate floor).
    let rate = compute_streaming_tok_per_sec(40, 10.0).expect("slow but credible");
    assert!((rate - 4.0).abs() < 0.001, "got {rate}");
}
