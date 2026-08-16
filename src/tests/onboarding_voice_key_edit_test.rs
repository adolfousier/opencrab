//! Editing a seeded voice key field replaces the stored key (#1075).
//!
//! Three secret inputs in the voice step routed both typing and paste through
//! handlers that knew nothing about the seeded `__EXISTING_KEY__` marker: the
//! generic `handle_text_field`, and the per-field paste arms. Every other
//! secret input in the wizard clears the marker on the first edit; these three
//! appended to it, producing `__EXISTING_KEY__<typed>` and persisting it.
//!
//! Fixtures are synthetic and carry no real credentials.

use crate::tui::onboarding::{OnboardingStep, OnboardingWizard, VoiceField, voice};
use crate::tui::provider_selector::EXISTING_KEY_SENTINEL;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn seeded(field: VoiceField) -> OnboardingWizard {
    let mut wizard = OnboardingWizard::new();
    wizard.step = OnboardingStep::VoiceSetup;
    wizard.voice_field = field;
    wizard.stt_openai_compat_key_input = EXISTING_KEY_SENTINEL.to_string();
    wizard.tts_openai_compat_key_input = EXISTING_KEY_SENTINEL.to_string();
    wizard.tts_api_key_input = EXISTING_KEY_SENTINEL.to_string();
    wizard
}

fn press(wizard: &mut OnboardingWizard, c: char) {
    voice::handle_key(wizard, KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
}

#[test]
fn typing_into_a_seeded_tts_key_replaces_the_marker() {
    let mut wizard = seeded(VoiceField::TtsApiKey);
    press(&mut wizard, 's');
    press(&mut wizard, 'k');
    assert_eq!(wizard.tts_api_key_input, "sk");
}

#[test]
fn pasting_over_a_seeded_tts_key_replaces_the_marker() {
    // The exact path that put a marker-prefixed key on disk.
    let mut wizard = seeded(VoiceField::TtsApiKey);
    wizard.handle_paste("sk-pasted-key");
    assert_eq!(wizard.tts_api_key_input, "sk-pasted-key");
}

#[test]
fn the_openai_compatible_key_fields_behave_the_same() {
    let mut stt = seeded(VoiceField::SttOpenaiCompatKey);
    stt.handle_paste("sk-stt-compat");
    assert_eq!(stt.stt_openai_compat_key_input, "sk-stt-compat");

    let mut tts = seeded(VoiceField::TtsOpenaiCompatKey);
    press(&mut tts, 'x');
    assert_eq!(tts.tts_openai_compat_key_input, "x");
}

#[test]
fn backspace_clears_a_seeded_key_whole() {
    // The marker is one value, not a run of characters. Deleting one char would
    // leave `__EXISTING_KEY_`, which no longer reads as seeded and would be
    // saved as if it were a key.
    let mut wizard = seeded(VoiceField::TtsApiKey);
    voice::handle_key(
        &mut wizard,
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
    );
    assert!(wizard.tts_api_key_input.is_empty());
}

#[test]
fn an_ordinary_typed_key_is_still_appended_to() {
    // The clear must fire once, on the seeded marker, and never again.
    let mut wizard = seeded(VoiceField::TtsApiKey);
    press(&mut wizard, 'a');
    press(&mut wizard, 'b');
    press(&mut wizard, 'c');
    assert_eq!(wizard.tts_api_key_input, "abc");
}

#[test]
fn a_non_secret_voice_field_is_unaffected() {
    // `handle_text_field` also drives URLs and model names. Those never hold
    // the marker, so the new clear must be a no-op for them.
    let mut wizard = seeded(VoiceField::SttOpenaiCompatUrl);
    wizard.stt_openai_compat_base_url = "https://example.invalid".to_string();
    press(&mut wizard, '/');
    assert_eq!(
        wizard.stt_openai_compat_base_url,
        "https://example.invalid/"
    );
}
