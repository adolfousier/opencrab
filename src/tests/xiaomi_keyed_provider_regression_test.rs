//! Regression: Xiaomi MiMo is a keyed provider, and the THREE places that
//! encode "does Xiaomi need an API key" must stay in agreement.
//!
//! When Xiaomi was keyless during the collab, three independent flags said so:
//!   1. `ProviderConfigs::provider_registry()` — `requires_api_key` (drives
//!      `active_provider_and_model`, i.e. which provider is the active default).
//!   2. `utils::providers` `ProviderMeta::needs_api_key` (drives
//!      `configured_providers`, i.e. whether it shows as set up).
//!   3. The onboarding `ProviderInfo::key_label` (drives whether the wizard
//!      prompts for a key).
//!
//! If any one of these drifts back to "keyless", Xiaomi silently breaks: it
//! either becomes a key-less default that can't create, lists itself as
//! configured with no key, or skips the key prompt. These tests exercise the
//! BEHAVIOUR of each flag so a single reverted line is caught.

use crate::config::{ProviderConfig, ProviderConfigs};

fn xiaomi_cfg(api_key: Option<&str>) -> ProviderConfigs {
    ProviderConfigs {
        xiaomi: Some(ProviderConfig {
            enabled: true,
            api_key: api_key.map(str::to_string),
            default_model: Some("mimo-v2.5-pro".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn shadow_1_active_provider_gates_xiaomi_on_key() {
    // provider_registry().requires_api_key: an enabled Xiaomi with no key must
    // NOT win the active-provider election.
    assert_eq!(
        xiaomi_cfg(None).active_provider_and_model().0,
        "none",
        "enabled key-less Xiaomi must be skipped by active_provider_and_model"
    );
    assert_eq!(
        xiaomi_cfg(Some("sk-user")).active_provider_and_model().0,
        "xiaomi",
        "enabled keyed Xiaomi must be the active provider"
    );
}

#[test]
fn shadow_2_configured_providers_gates_xiaomi_on_key() {
    // ProviderMeta::needs_api_key: configured_providers lists a provider as set
    // up only when its key requirement is met.
    let keyless = crate::utils::providers::configured_providers(&xiaomi_cfg(None));
    assert!(
        !keyless.iter().any(|(id, _)| id == "xiaomi"),
        "key-less Xiaomi must NOT report as configured, got {keyless:?}"
    );
    let keyed = crate::utils::providers::configured_providers(&xiaomi_cfg(Some("sk-user")));
    assert!(
        keyed.iter().any(|(id, _)| id == "xiaomi"),
        "keyed Xiaomi must report as configured, got {keyed:?}"
    );
}

#[test]
fn shadow_3_onboarding_entry_prompts_for_a_key() {
    // ProviderInfo::key_label: a non-empty label is what makes the wizard show
    // the API-key field instead of skipping it.
    let xiaomi = crate::tui::onboarding::PROVIDERS
        .iter()
        .find(|p| p.id == "xiaomi")
        .expect("xiaomi in onboarding PROVIDERS");
    assert!(
        !xiaomi.key_label.is_empty(),
        "Xiaomi's onboarding key_label must be non-empty so the wizard prompts for a key"
    );
}

#[test]
fn default_base_url_points_at_official_xiaomi_endpoint() {
    // The keyed provider must default to Xiaomi's real OpenAI-compatible
    // endpoint, not the retired collab proxy.
    let src = include_str!("../brain/provider/factory.rs");
    assert!(
        src.contains("https://api.xiaomimimo.com/v1/chat/completions"),
        "factory must default Xiaomi's base_url to the official xiaomimimo endpoint"
    );
    assert!(
        !src.contains("xiaomi-collab.opencrabs.com"),
        "the retired collab proxy URL must not remain in the factory"
    );
}
