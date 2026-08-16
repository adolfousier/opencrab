//! A reopened wizard carries the voice config it is about to overwrite.
//!
//! `apply_config` writes `providers.stt.*.enabled` and `providers.tts.*.enabled`
//! from wizard state on every full save, and those fields hold plain values,
//! not "untouched" markers: a wizard built by `new()` reports
//! `SttProvider::Off` because that is the struct default, not because the user
//! chose it. `resumed()` used to build on `new()`, so reopening the wizard and
//! leaving it wrote `enabled = false` over working STT and TTS config. The
//! symptom was voice notes that worked right after setup and were disabled
//! again the next time the wizard was opened.
//!
//! `resumed()` itself reads `~/.opencrabs/config.toml`, so it is not exercised
//! here. What is exercised is the seeding path it now goes through, which is
//! where the round-trip either holds or does not.

use crate::config::{Config, ProviderConfig, SttProviders, TtsProviders};
use crate::tui::onboarding::{OnboardingWizard, SttProvider, TtsProvider};

fn config_with_voice(stt_groq: bool, tts_openai: bool) -> Config {
    let mut config = Config::default();
    config.providers.stt = Some(SttProviders {
        groq: Some(ProviderConfig {
            enabled: stt_groq,
            ..Default::default()
        }),
        ..Default::default()
    });
    config.providers.tts = Some(TtsProviders {
        openai: Some(ProviderConfig {
            enabled: tts_openai,
            ..Default::default()
        }),
        ..Default::default()
    });
    config
}

#[test]
fn a_fresh_wizard_reports_voice_as_off() {
    // Not a bug on its own — it is the correct default for a first run. It is
    // only destructive when a wizard in this state reaches a save, which is
    // what building `resumed()` on it used to do.
    let wizard = OnboardingWizard::new();
    assert_eq!(wizard.stt_provider, SttProvider::Off);
    assert_eq!(wizard.tts_provider, TtsProvider::Off);
}

#[test]
fn seeding_from_config_preserves_enabled_voice_providers() {
    let wizard = OnboardingWizard::from_config(&config_with_voice(true, true));
    assert_eq!(
        wizard.stt_provider,
        SttProvider::Groq,
        "an enabled STT provider must survive reopening the wizard"
    );
    assert_eq!(
        wizard.tts_provider,
        TtsProvider::OpenAi,
        "an enabled TTS provider must survive reopening the wizard"
    );
}

#[test]
fn seeding_from_config_keeps_disabled_voice_off() {
    // The other direction has to hold too, or reopening the wizard would
    // silently turn voice on for someone who never asked for it.
    let wizard = OnboardingWizard::from_config(&config_with_voice(false, false));
    assert_eq!(wizard.stt_provider, SttProvider::Off);
    assert_eq!(wizard.tts_provider, TtsProvider::Off);
}
