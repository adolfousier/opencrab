//! RSI's session must follow config, not outlive it (#805).
//!
//! The RSI session is persistent and was created months ago on whatever
//! provider was active then. `ensure_session_provider_restored` (#704) restores
//! the SESSION's saved provider at turn start, which is correct for a user
//! session and wrong here, so `self_improvement_provider` had no effect for
//! months.
//!
//! The observed consequence was worse than running on an unintended model: the
//! pinned provider was a CLI provider, whose `cli_handles_tools()` makes the
//! tool loop skip local execution entirely. RSI's only purpose is calling
//! `feedback_analyze`, `self_improve` and `rsi_propose`, so every cycle was a
//! silent, paid no-op.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::brain::rsi::rsi_pair_is_stale;

#[test]
fn a_pair_matching_config_is_not_stale() {
    assert!(!rsi_pair_is_stale(
        Some("modelstudio"),
        Some("qwen3.8-max-preview"),
        "modelstudio",
        Some("qwen3.8-max-preview")
    ));
}

#[test]
fn the_observed_stale_pin_is_detected() {
    // The real case: session pinned to a CLI provider in April, config since
    // changed to an API provider.
    assert!(rsi_pair_is_stale(
        Some("claude-cli"),
        Some("fable"),
        "modelstudio",
        Some("qwen3.8-max-preview")
    ));
}

#[test]
fn a_changed_model_alone_is_stale() {
    // Config authority covers the model too, or switching model silently keeps
    // the old one.
    assert!(rsi_pair_is_stale(
        Some("modelstudio"),
        Some("qwen3.6-max-preview"),
        "modelstudio",
        Some("qwen3.8-max-preview")
    ));
}

#[test]
fn an_unset_model_is_not_a_disagreement() {
    // Leaving self_improvement_model unset means "use the provider's default",
    // so the stored model must be left alone rather than cleared.
    assert!(!rsi_pair_is_stale(
        Some("modelstudio"),
        Some("some-default-model"),
        "modelstudio",
        None
    ));
}

#[test]
fn a_session_with_no_provider_is_stale() {
    // Several of the duplicate RSI sessions carry a provider but no model, and
    // one carries neither. An unpinned session must adopt config rather than
    // fall through to the global default.
    assert!(rsi_pair_is_stale(None, None, "modelstudio", None));
    assert!(rsi_pair_is_stale(
        Some("modelstudio"),
        None,
        "modelstudio",
        Some("qwen3.8-max-preview")
    ));
}
