//! Only a turn that stayed on the session's provider may rewrite its model.

use crate::brain::agent::service::should_refresh_session_model;

#[test]
fn an_alias_resolving_to_a_concrete_version_is_adopted() {
    // Why the refresh exists: ask a CLI for `opus` and it answers as the
    // version it resolved, which the footer should show without a restart.
    assert!(should_refresh_session_model(true, "opus", "claude-opus-5"));
}

#[test]
fn a_turn_that_ran_elsewhere_cannot_replace_the_pick() {
    // The bug: the turn fell to another provider, reported that provider's
    // model, and the override took it, so the next turn used a model the user
    // never chose and `/models` looked ignored.
    assert!(
        !should_refresh_session_model(false, "Qwen3.8-27B", "mimo-v2.5-pro"),
        "a provider the session did not choose cannot describe its choice"
    );
}

#[test]
fn an_unchanged_model_is_not_rewritten() {
    assert!(!should_refresh_session_model(
        true,
        "claude-opus-5",
        "claude-opus-5"
    ));
}

#[test]
fn a_provider_reporting_nothing_cannot_blank_the_pick() {
    assert!(!should_refresh_session_model(true, "Qwen3.8-27B", ""));
    assert!(!should_refresh_session_model(true, "Qwen3.8-27B", "   "));
}
