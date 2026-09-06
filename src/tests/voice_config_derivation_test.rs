//! `Config::voice_config()` derives the runtime voice view from the
//! `[providers.stt.*]` / `[providers.tts.*]` engine blocks. A disabled block
//! must contribute nothing (#1399): the dispatcher chooses providers by the
//! presence of the derived fields, so a switched-off openai_compatible entry
//! that still carried its base_url was dispatched first on every voice note
//! and failed every time, while the enabled provider waited behind it.

use crate::config::Config;

fn cfg(toml: &str) -> Config {
    toml::from_str(toml).expect("config toml parses")
}

#[test]
fn disabled_openai_compatible_tts_contributes_no_base_url_or_key() {
    let vc = cfg(r#"
[providers.tts.openai_compatible]
enabled = false
base_url = "http://localhost:11434"
model = "tts-1"
api_key = "compat-key"
[providers.tts.openai]
enabled = true
api_key = "openai-key"
"#)
    .voice_config();
    assert_eq!(vc.tts_base_url, None, "disabled block leaked its base_url");
    assert_eq!(vc.tts_api_key.as_deref(), Some("openai-key"));
    assert_eq!(
        vc.tts_provider.as_ref().and_then(|p| p.api_key.as_deref()),
        Some("openai-key")
    );
    assert!(vc.tts_enabled);
}

#[test]
fn enabled_openai_compatible_tts_is_the_derived_endpoint() {
    let vc = cfg(r#"
[providers.tts.openai_compatible]
enabled = true
base_url = "http://localhost:11434"
model = "tts-1"
api_key = "compat-key"
"#)
    .voice_config();
    assert_eq!(vc.tts_base_url.as_deref(), Some("http://localhost:11434"));
    assert_eq!(vc.tts_api_key.as_deref(), Some("compat-key"));
    assert_eq!(vc.tts_model, "tts-1");
}

#[test]
fn disabled_openai_tts_with_a_stored_key_is_not_a_provider() {
    let vc = cfg(r#"
[providers.tts.openai]
enabled = false
api_key = "openai-key"
"#)
    .voice_config();
    assert!(
        vc.tts_provider.is_none(),
        "disabled openai must not be a candidate"
    );
    assert_eq!(vc.tts_api_key, None);
    assert_eq!(vc.tts_base_url, None, "no synthetic api.openai.com URL");
    assert!(!vc.tts_enabled);
}

#[test]
fn openai_tts_alone_does_not_plant_a_base_url() {
    // The OpenAI kind never reads tts_base_url; planting api.openai.com
    // there made the dispatcher label real OpenAI calls openai_compatible.
    let vc = cfg(r#"
[providers.tts.openai]
enabled = true
api_key = "openai-key"
"#)
    .voice_config();
    assert_eq!(vc.tts_base_url, None);
    assert!(vc.tts_provider.is_some());
}

#[test]
fn disabled_groq_stt_with_a_stored_key_is_not_a_provider() {
    let vc = cfg(r#"
[providers.stt.groq]
enabled = false
api_key = "groq-key"
"#)
    .voice_config();
    assert!(vc.stt_provider.is_none());
    assert_eq!(vc.stt_api_key, None);
    assert_eq!(vc.stt_base_url, None, "no synthetic groq URL");
}

#[test]
fn disabled_openai_compatible_stt_contributes_nothing_next_to_enabled_groq() {
    let vc = cfg(r#"
[providers.stt.openai_compatible]
enabled = false
base_url = "http://localhost:11434"
model = "whisper-large-v3-turbo"
api_key = "compat-key"
[providers.stt.groq]
enabled = true
api_key = "groq-key"
"#)
    .voice_config();
    assert_eq!(vc.stt_base_url, None);
    assert_eq!(vc.stt_api_key.as_deref(), Some("groq-key"));
    assert!(vc.stt_provider.is_some());
}

#[test]
fn chains_and_display_preferences_survive_regardless_of_enabled() {
    // The voice/model the wizard seeds its fields from are preferences,
    // not candidates; they stay readable while the engine is off.
    let vc = cfg(r#"
[providers.tts]
fallback_chain = ["openai", "local"]
[providers.tts.openai]
enabled = false
voice = "echo"
model = "gpt-4o-mini-tts"
[providers.stt]
fallback_chain = ["local", "groq"]
"#)
    .voice_config();
    assert_eq!(vc.tts_fallback_chain, vec!["openai", "local"]);
    assert_eq!(vc.stt_fallback_chain, vec!["local", "groq"]);
    assert_eq!(vc.tts_voice, "echo");
    assert_eq!(vc.tts_model, "gpt-4o-mini-tts");
}
