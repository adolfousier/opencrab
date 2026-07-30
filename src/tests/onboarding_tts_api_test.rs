//! Tests for the built-in OpenAI TTS voice selector and API key field
//! in the onboarding wizard (issues #874, #875).

use crate::tui::onboarding::voice::OPENAI_TTS_VOICES;
use crate::tui::onboarding::{OnboardingStep, OnboardingWizard, TtsProvider, VoiceField};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::empty())
}

fn voice_wizard() -> OnboardingWizard {
    let mut w = OnboardingWizard::new();
    w.step = OnboardingStep::VoiceSetup;
    w.tts_provider = TtsProvider::OpenAi;
    w.tts_enabled = true;
    w.voice_field = VoiceField::TtsApiVoiceSelect;
    w
}

// ── #874: Voice selector navigation ───────────────────────────

#[test]
fn tts_api_voice_default_is_echo() {
    let w = voice_wizard();
    assert_eq!(w.tts_api_voice, "echo");
}

#[test]
fn tts_api_voice_down_advances() {
    let mut w = voice_wizard();
    // "echo" is index 4, Down should go to "fable" (index 5)
    w.handle_key(key(KeyCode::Down));
    assert_eq!(w.tts_api_voice, "fable");
}

#[test]
fn tts_api_voice_up_goes_back() {
    let mut w = voice_wizard();
    // "echo" is index 4, Up should go to "coral" (index 3)
    w.handle_key(key(KeyCode::Up));
    assert_eq!(w.tts_api_voice, "coral");
}

#[test]
fn tts_api_voice_up_at_top_stays() {
    let mut w = voice_wizard();
    w.tts_api_voice = "alloy".to_string(); // index 0
    w.handle_key(key(KeyCode::Up));
    assert_eq!(w.tts_api_voice, "alloy");
}

#[test]
fn tts_api_voice_down_at_bottom_stays() {
    let mut w = voice_wizard();
    w.tts_api_voice = "shimmer".to_string(); // last index
    w.handle_key(key(KeyCode::Down));
    assert_eq!(w.tts_api_voice, "shimmer");
}

#[test]
fn tts_api_voice_enter_advances_to_key() {
    let mut w = voice_wizard();
    w.handle_key(key(KeyCode::Enter));
    assert_eq!(w.voice_field, VoiceField::TtsApiKey);
}

#[test]
fn tts_api_voice_tab_advances_to_key() {
    let mut w = voice_wizard();
    w.handle_key(key(KeyCode::Tab));
    assert_eq!(w.voice_field, VoiceField::TtsApiKey);
}

#[test]
fn tts_api_voice_backtab_goes_to_mode_select() {
    let mut w = voice_wizard();
    w.handle_key(key(KeyCode::BackTab));
    assert_eq!(w.voice_field, VoiceField::TtsModeSelect);
}

// ── #875: API key field navigation ────────────────────────────

#[test]
fn tts_api_key_enter_advances_to_continue() {
    let mut w = voice_wizard();
    w.voice_field = VoiceField::TtsApiKey;
    w.handle_key(key(KeyCode::Enter));
    assert_eq!(w.voice_field, VoiceField::Continue);
}

#[test]
fn tts_api_key_backtab_goes_to_voice() {
    let mut w = voice_wizard();
    w.voice_field = VoiceField::TtsApiKey;
    w.handle_key(key(KeyCode::BackTab));
    assert_eq!(w.voice_field, VoiceField::TtsApiVoiceSelect);
}

#[test]
fn tts_api_key_char_input_appends() {
    let mut w = voice_wizard();
    w.voice_field = VoiceField::TtsApiKey;
    w.tts_api_key_input = String::new();
    w.handle_key(key(KeyCode::Char('s')));
    w.handle_key(key(KeyCode::Char('k')));
    assert_eq!(w.tts_api_key_input, "sk");
}

#[test]
fn tts_api_key_backspace_pops() {
    let mut w = voice_wizard();
    w.voice_field = VoiceField::TtsApiKey;
    w.tts_api_key_input = "sk-test".to_string();
    w.handle_key(key(KeyCode::Backspace));
    assert_eq!(w.tts_api_key_input, "sk-tes");
}

// ── Navigation flow: advance_from_tts ─────────────────────────

#[test]
fn advance_from_tts_openai_goes_to_voice_select() {
    let mut w = OnboardingWizard::new();
    w.step = OnboardingStep::VoiceSetup;
    w.voice_field = VoiceField::TtsModeSelect;
    w.tts_provider = TtsProvider::OpenAi;
    // Simulate selecting OpenAI TTS mode (Enter on TtsModeSelect)
    w.handle_key(key(KeyCode::Enter));
    assert_eq!(w.voice_field, VoiceField::TtsApiVoiceSelect);
}

#[test]
fn continue_backtab_openai_goes_to_key() {
    let mut w = voice_wizard();
    w.voice_field = VoiceField::Continue;
    w.handle_key(key(KeyCode::BackTab));
    assert_eq!(w.voice_field, VoiceField::TtsApiKey);
}

// ── Voice list sanity ─────────────────────────────────────────

#[test]
fn openai_tts_voices_has_ten_entries() {
    assert_eq!(OPENAI_TTS_VOICES.len(), 10);
}

#[test]
fn openai_tts_voices_contains_echo() {
    assert!(OPENAI_TTS_VOICES.contains(&"echo"));
}
