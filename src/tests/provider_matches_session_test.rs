//! #705: `provider_matches_session` decides whether a turn started on the
//! session's own provider — the gate that keeps an involuntary provider remap
//! from being persisted over a session's saved choice.

use crate::brain::agent::service::provider_matches_session;

#[test]
fn no_saved_provider_counts_as_matched() {
    // A fresh session captures its first pair legitimately.
    assert!(provider_matches_session(None, "modelstudio"));
}

#[test]
fn same_provider_matches() {
    assert!(provider_matches_session(Some("modelstudio"), "modelstudio"));
}

#[test]
fn different_provider_does_not_match() {
    // The exact silent-switch shape: saved modelstudio, turn ran on modelscope
    // (global default after a restart) — must NOT count as matched, so the
    // remapped pair is never persisted.
    assert!(!provider_matches_session(Some("modelstudio"), "modelscope"));
}
