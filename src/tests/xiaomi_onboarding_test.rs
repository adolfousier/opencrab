//! Regression: keyless API providers (Xiaomi) must onboard without an API key.
//!
//! Xiaomi is a key-LESS API provider (empty key_label, the proxy supplies the
//! key). Selecting it in onboarding must skip the API-key field and go straight
//! to model selection with a populated model list — otherwise the user gets
//! stuck on the key field and can never enable the provider.

use crate::tui::onboarding::{AuthField, OnboardingStep, OnboardingWizard, PROVIDERS};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[test]
fn xiaomi_keyless_skips_api_key_and_loads_models() {
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

    // Keyless provider must SKIP the API-key field and land on Model.
    assert_eq!(
        wizard.auth_field,
        AuthField::Model,
        "Xiaomi (empty key_label) must skip the API-key field, not get stuck on ApiKey"
    );

    // And there must be a selectable model list, or the user can't pick a model
    // and complete onboarding.
    let has_models = !wizard.ps.config_models.is_empty() || !wizard.ps.models.is_empty();
    assert!(
        has_models,
        "Xiaomi must have a selectable model list after skipping the key field; \
         config_models={:?} models={:?}",
        wizard.ps.config_models, wizard.ps.models
    );
}
