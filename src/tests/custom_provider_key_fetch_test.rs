//! Custom-provider model fetch must send the real saved API key (#656).
//!
//! When a provider already has a key, `api_key_input` holds the
//! `EXISTING_KEY_SENTINEL` placeholder, not the real key. The onboarding and
//! `/models` fetch sites used to send that placeholder (or nothing) as the
//! bearer token, so a remote custom endpoint whose key lives in keys.toml
//! (e.g. Model Studio) got 401 while the chat path authenticated fine.
//!
//! `effective_api_key` resolves the real key: a freshly typed one, else the
//! saved key from config, and NEVER the sentinel.

use crate::tui::provider_selector::{
    CUSTOM_PROVIDER_IDX, EXISTING_KEY_SENTINEL, ProviderSelectorState,
};

fn ps_with(api_key_input: &str) -> ProviderSelectorState {
    ProviderSelectorState {
        // CUSTOM_PROVIDER_IDX is the "add custom" meta row: load_api_key_from_config
        // resolves no saved key for it, so the config-fallback branch is a
        // deterministic None in tests (no dependence on the machine's keys.toml).
        selected_provider: CUSTOM_PROVIDER_IDX,
        api_key_input: api_key_input.to_string(),
        ..Default::default()
    }
}

#[test]
fn freshly_typed_key_is_used_verbatim() {
    let ps = ps_with("sk-real-abc123");
    assert_eq!(ps.effective_api_key().as_deref(), Some("sk-real-abc123"));
}

#[test]
fn sentinel_is_never_sent_as_the_key() {
    let ps = ps_with(EXISTING_KEY_SENTINEL);
    // No saved key at this index, so it resolves to None — but the crucial
    // guarantee is that the placeholder never leaks out as a bearer token.
    assert_ne!(
        ps.effective_api_key().as_deref(),
        Some(EXISTING_KEY_SENTINEL)
    );
    assert_eq!(ps.effective_api_key(), None);
}

#[test]
fn empty_input_falls_back_to_config() {
    let ps = ps_with("");
    // Falls through to the config lookup (None here), never a bogus token.
    assert_eq!(ps.effective_api_key(), None);
}
