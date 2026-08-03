//! Regression (#919): onboarding runs until the user finishes it.
//!
//! The gate used to ask "could some provider serve a request right now?".
//! CLI providers need no API key, so one `enabled = true` section — left by a
//! partial run or a hand edit — answered yes and the wizard never appeared,
//! dropping a first-time user into a chat with no usable pair. Completion is
//! now a recorded fact, and progress is recorded as the user moves so leaving
//! halfway resumes rather than restarts.

use crate::config::profile::with_home_override;
use crate::tui::onboarding::state::OnboardingState;
use crate::tui::onboarding::{OnboardingStep, WizardMode, is_first_time};

/// A tempdir laid out as an `.opencrabs` home, optionally seeded with a
/// config.toml. Scoped with `with_home_override` rather than `$HOME` so the
/// test cannot be disturbed by, or disturb, anything else in the suite (#912).
fn temp_home(config_toml: Option<&str>) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let opencrabs = dir.path().join(".opencrabs");
    std::fs::create_dir_all(&opencrabs).expect("create .opencrabs");
    if let Some(toml) = config_toml {
        std::fs::write(opencrabs.join("config.toml"), toml).expect("write config");
        std::fs::write(opencrabs.join("keys.toml"), b"").expect("write keys");
    }
    (dir, opencrabs)
}

/// A CLI provider that is enabled but has no model chosen — the exact shape
/// that used to be mistaken for a finished setup.
const ENABLED_CLI_ONLY: &str = r#"
[providers.claude_cli]
enabled = true
"#;

/// The same provider with a model actually chosen: a real finished setup.
const ENABLED_CLI_WITH_MODEL: &str = r#"
[providers.claude_cli]
enabled = true
default_model = "sonnet"
"#;

#[test]
fn a_detected_cli_provider_alone_does_not_count_as_onboarded() {
    let (_temp, home) = temp_home(Some(ENABLED_CLI_ONLY));
    with_home_override(home, || {
        assert!(
            is_first_time(),
            "an enabled CLI provider with no model chosen is not a completed setup"
        );
    });
}

#[test]
fn onboarding_reappears_until_it_is_finished() {
    let (_temp, home) = temp_home(Some(ENABLED_CLI_ONLY));
    with_home_override(home, || {
        assert!(is_first_time(), "first start");
        // Getting partway through and leaving must not count as finishing.
        OnboardingState::record_step(OnboardingStep::ProviderAuth.as_key(), "quick");
        assert!(is_first_time(), "second start, still unfinished");

        OnboardingState::mark_completed();
        assert!(!is_first_time(), "finished — must not be asked again");
    });
}

#[test]
fn a_genuinely_finished_install_is_never_nagged() {
    // No progress file at all, which is every install predating it. A config
    // with a provider AND a chosen model is a finished setup, so it must be
    // recorded rather than sent back through the wizard.
    let (_temp, home) = temp_home(Some(ENABLED_CLI_WITH_MODEL));
    with_home_override(home.clone(), || {
        assert!(!OnboardingState::path().exists(), "precondition: no marker");
        assert!(
            !is_first_time(),
            "a complete setup must not re-run onboarding"
        );
        assert!(
            OnboardingState::load().completed,
            "the migration must record its verdict so it is decided once"
        );
    });
}

#[test]
fn a_missing_config_is_never_mistaken_for_a_finished_setup() {
    let (_temp, home) = temp_home(None);
    with_home_override(home, || {
        assert!(is_first_time(), "nothing configured at all");
        assert!(
            !OnboardingState::load().completed,
            "the migration must not mark an empty install complete"
        );
    });
}

#[test]
fn progress_survives_a_restart() {
    let (_temp, home) = temp_home(Some(ENABLED_CLI_ONLY));
    with_home_override(home, || {
        OnboardingState::record_step(OnboardingStep::VoiceSetup.as_key(), "advanced");

        let reloaded = OnboardingState::load();
        assert_eq!(reloaded.last_step.as_deref(), Some("voice_setup"));
        assert_eq!(
            reloaded.mode.as_deref(),
            Some("advanced"),
            "which steps remain depends on the flow, so the flow is recorded too"
        );
        assert!(!reloaded.completed);
    });
}

#[test]
fn completing_clears_the_resume_point() {
    let (_temp, home) = temp_home(Some(ENABLED_CLI_ONLY));
    with_home_override(home, || {
        OnboardingState::record_step(OnboardingStep::Daemon.as_key(), "quick");
        OnboardingState::mark_completed();

        let state = OnboardingState::load();
        assert!(state.completed);
        assert!(
            state.last_step.is_none(),
            "a finished run has nothing left to resume"
        );
    });
}

#[test]
fn step_keys_round_trip() {
    // Persisted keys must survive a restart, so every step maps back to itself.
    for step in OnboardingStep::flow(WizardMode::Advanced) {
        assert_eq!(
            OnboardingStep::from_key(step.as_key()),
            Some(*step),
            "{} must round-trip",
            step.as_key()
        );
    }
    assert_eq!(
        OnboardingStep::from_key("a_step_from_some_other_build"),
        None,
        "an unknown key restarts the wizard rather than resuming somewhere arbitrary"
    );
}

#[test]
fn remaining_steps_are_reported_for_the_flow_the_user_chose() {
    // Quick skips Channels, Voice and Image, so the outstanding list must not
    // promise steps that flow will never show.
    let quick = OnboardingStep::ProviderAuth.remaining_titles(WizardMode::QuickStart);
    assert_eq!(
        quick,
        vec![
            OnboardingStep::Daemon.title(),
            OnboardingStep::HealthCheck.title(),
            OnboardingStep::BrainSetup.title(),
        ]
    );

    let advanced = OnboardingStep::ProviderAuth.remaining_titles(WizardMode::Advanced);
    assert!(advanced.contains(&OnboardingStep::Channels.title()));
    assert!(advanced.contains(&OnboardingStep::VoiceSetup.title()));

    // A channel sub-step counts from Channels, its parent.
    assert_eq!(
        OnboardingStep::TelegramSetup.remaining_titles(WizardMode::Advanced),
        OnboardingStep::Channels.remaining_titles(WizardMode::Advanced),
    );

    // The last step has nothing after it.
    assert!(
        OnboardingStep::BrainSetup
            .remaining_titles(WizardMode::QuickStart)
            .is_empty()
    );
}
