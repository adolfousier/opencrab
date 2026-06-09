//! Regression: keyless API providers (Xiaomi) must onboard without an API key.
//!
//! Xiaomi is a key-LESS API provider (empty key_label, the proxy supplies the
//! key). Selecting it in onboarding must skip the API-key field and go straight
//! to model selection with a populated model list — otherwise the user gets
//! stuck on the key field and can never enable the provider.

use crate::tui::onboarding::{
    AuthField, OnboardingStep, OnboardingWizard, PROVIDERS, WizardAction,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn enter() -> KeyEvent {
    KeyEvent::new(KeyCode::Enter, KeyModifiers::empty())
}

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

/// Full quick-jump flow (what /onboard:provider and /models do): select Xiaomi
/// on the provider field, Enter to the model field, Enter on the model must
/// COMMIT (return QuickJumpDone) — previously a no-op / stuck.
#[test]
#[allow(clippy::field_reassign_with_default)]
fn xiaomi_keyless_quickjump_full_flow_commits() {
    let mut w = OnboardingWizard::default();
    w.quick_jump = true;
    w.step = OnboardingStep::ProviderAuth;
    w.auth_field = AuthField::Provider;
    let idx = PROVIDERS
        .iter()
        .position(|p| p.id == "xiaomi")
        .expect("xiaomi in PROVIDERS");
    w.ps.selected_provider = idx;

    // Enter on the provider field → keyless skip to Model with models loaded.
    let _ = w.handle_key(enter());
    assert_eq!(w.auth_field, AuthField::Model);
    assert!(
        !w.ps.config_models.is_empty() || !w.ps.models.is_empty(),
        "model list must be populated"
    );

    // Enter on the model field → must commit.
    let action = w.handle_key(enter());
    assert_eq!(
        action,
        WizardAction::QuickJumpDone,
        "Enter on the model field must commit the keyless Xiaomi selection"
    );
}

/// /models populates its list via fetch_provider_models — it must return the
/// mimo chat models for Xiaomi (was empty: xiaomi missing from the endpoint
/// match, so the picker showed nothing to select).
#[tokio::test]
async fn xiaomi_models_fetch_returns_mimo_list() {
    let idx = PROVIDERS
        .iter()
        .position(|p| p.id == "xiaomi")
        .expect("xiaomi in PROVIDERS");
    let models = crate::tui::onboarding::fetch_provider_models(idx, None, None, None).await;
    assert!(
        !models.is_empty(),
        "/models must show a model list for Xiaomi (was empty)"
    );
    assert!(
        models.iter().any(|m| m.contains("mimo")),
        "expected mimo models, got {models:?}"
    );
}
