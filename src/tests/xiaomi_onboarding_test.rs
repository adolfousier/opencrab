//! Regression: Xiaomi MiMo is a KEYED provider (it used to be keyless during
//! the collab). Selecting it in onboarding must land on the API-key field, and
//! an enabled-but-keyless section must NOT resolve as the active provider — the
//! `requires_api_key` gate keeps a key-less Xiaomi from becoming a broken
//! default.

use crate::tui::onboarding::{AuthField, OnboardingStep, OnboardingWizard, PROVIDERS};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[test]
fn xiaomi_keyed_lands_on_api_key_field() {
    let mut wizard = OnboardingWizard::new();
    let idx = PROVIDERS
        .iter()
        .position(|p| p.id == "xiaomi")
        .expect("xiaomi present in onboarding PROVIDERS");

    wizard.step = OnboardingStep::ProviderAuth;
    wizard.auth_field = AuthField::Provider;
    wizard.ps.selected_provider = idx;

    // Confirm the provider selection (Enter on the provider field).
    let _ = wizard.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

    // With the endpoint_type toggle, selecting xiaomi lands on XiaomiEndpointType first.
    // User picks API or Token Plan, then advances to the API-key field.
    assert_eq!(
        wizard.auth_field,
        AuthField::XiaomiEndpointType,
        "Xiaomi has an endpoint_type toggle — selecting it must land on XiaomiEndpointType first"
    );

    // Advance past the endpoint type toggle (Tab/Enter)
    let _ = wizard.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::empty()));
    assert_eq!(
        wizard.auth_field,
        AuthField::ApiKey,
        "Tabbing from XiaomiEndpointType must land on the API-key field"
    );
}

/// The onboarding entry for Xiaomi must declare a key field, so the wizard
/// prompts for a key instead of treating it as keyless.
#[test]
fn xiaomi_provider_entry_requires_a_key() {
    let xiaomi = PROVIDERS
        .iter()
        .find(|p| p.id == "xiaomi")
        .expect("xiaomi in PROVIDERS");
    assert!(
        !xiaomi.key_label.is_empty(),
        "Xiaomi must have a non-empty key_label so onboarding prompts for a key"
    );
}

/// An enabled Xiaomi section with NO api_key must NOT resolve as the active
/// provider — `requires_api_key` skips it so it never becomes a broken default.
#[test]
#[allow(clippy::field_reassign_with_default)]
fn xiaomi_enabled_without_key_is_not_active() {
    use crate::config::{ProviderConfig, ProviderConfigs};
    let mut providers = ProviderConfigs::default();
    providers.xiaomi = Some(ProviderConfig {
        enabled: true,
        default_model: Some("mimo-v2.5-pro".to_string()),
        ..Default::default()
    });
    let (provider, _) = providers.active_provider_and_model();
    assert_eq!(
        provider, "none",
        "an enabled but key-less Xiaomi must be skipped, not selected as active"
    );
}

/// An enabled Xiaomi section WITH an api_key resolves as the active provider.
#[test]
#[allow(clippy::field_reassign_with_default)]
fn xiaomi_enabled_with_key_is_active() {
    use crate::config::{ProviderConfig, ProviderConfigs};
    let mut providers = ProviderConfigs::default();
    providers.xiaomi = Some(ProviderConfig {
        enabled: true,
        api_key: Some("sk-user-key".to_string()),
        default_model: Some("mimo-v2.5-pro".to_string()),
        ..Default::default()
    });
    let (provider, model) = providers.active_provider_and_model();
    assert_eq!(provider, "xiaomi", "a keyed, enabled Xiaomi must be active");
    assert_eq!(model, "mimo-v2.5-pro");
}

/// /models populates its list via fetch_provider_models — it must return the
/// mimo chat models for Xiaomi (static fallback list) so the picker has
/// something to select even before a key is entered.
#[tokio::test]
async fn xiaomi_models_fetch_returns_mimo_list() {
    let idx = PROVIDERS
        .iter()
        .position(|p| p.id == "xiaomi")
        .expect("xiaomi in PROVIDERS");
    let models =
        crate::tui::onboarding::fetch_provider_models(idx, None, None, None, None, None).await;
    assert!(
        !models.is_empty(),
        "/models must show a model list for Xiaomi (was empty)"
    );
    assert!(
        models.iter().any(|m| m.contains("mimo")),
        "expected mimo models, got {models:?}"
    );
}
