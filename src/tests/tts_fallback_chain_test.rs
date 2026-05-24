//! TTS fallback chain dispatcher tests.
//!
//! Mirror of `stt_fallback_chain_test`: exercises
//! `voice::service::resolve_tts_fallback_chain` to confirm the
//! user-configured `tts_fallback_chain` produces the right sequence of
//! provider attempts when the primary is failing.

use crate::channels::voice::service::TtsProviderKind;
use crate::channels::voice::service::resolve_tts_fallback_chain;
use crate::config::{ProviderConfig, VoiceConfig};

fn voicebox_primary_config() -> VoiceConfig {
    VoiceConfig {
        voicebox_tts_enabled: true,
        voicebox_tts_base_url: "http://localhost:8000".to_string(),
        voicebox_tts_profile_id: "profile-abc".to_string(),
        voicebox_tts_engine: "xtts".to_string(),
        tts_base_url: Some("https://api.openai.com/v1/audio/speech".to_string()),
        tts_api_key: Some("sk-test".to_string()),
        tts_provider: Some(ProviderConfig {
            api_key: Some("openai-key".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn empty_chain_uses_default_priority_with_primary_skipped() {
    let cfg = voicebox_primary_config();
    let chain = resolve_tts_fallback_chain(&cfg, TtsProviderKind::Voicebox);
    let labels: Vec<String> = chain.iter().map(|k| k.label()).collect();
    assert!(
        labels.contains(&"openai_compatible".to_string()),
        "default chain should include openai_compatible when configured, got {labels:?}",
    );
    assert!(
        labels.contains(&"openai".to_string()),
        "default chain should include openai when configured, got {labels:?}",
    );
    assert!(
        !labels.contains(&"voicebox".to_string()),
        "primary must be excluded from its own fallback chain, got {labels:?}",
    );
}

#[test]
fn user_chain_order_is_respected() {
    let cfg = VoiceConfig {
        tts_fallback_chain: vec!["openai".into(), "openai_compatible".into()],
        ..voicebox_primary_config()
    };
    let chain = resolve_tts_fallback_chain(&cfg, TtsProviderKind::Voicebox);
    let labels: Vec<String> = chain.iter().map(|k| k.label()).collect();
    assert_eq!(labels, vec!["openai", "openai_compatible"]);
}

#[test]
fn unconfigured_entries_are_filtered_out() {
    // Only voicebox has any config — and voicebox IS the primary so it's
    // also excluded. The chain should be empty.
    let cfg = VoiceConfig {
        voicebox_tts_enabled: true,
        voicebox_tts_base_url: "http://localhost:8000".to_string(),
        tts_fallback_chain: vec![
            "voicebox".into(),
            "openai_compatible".into(),
            "openai".into(),
        ],
        ..Default::default()
    };
    let chain = resolve_tts_fallback_chain(&cfg, TtsProviderKind::Voicebox);
    assert!(chain.is_empty(), "unconfigured providers must be skipped");
}

#[test]
fn primary_is_never_re_attempted_via_chain() {
    let cfg = VoiceConfig {
        tts_fallback_chain: vec!["voicebox".into(), "voicebox".into(), "openai".into()],
        ..voicebox_primary_config()
    };
    let chain = resolve_tts_fallback_chain(&cfg, TtsProviderKind::Voicebox);
    let labels: Vec<String> = chain.iter().map(|k| k.label()).collect();
    assert!(
        !labels.contains(&"voicebox".to_string()),
        "primary must be skipped even when the user lists it explicitly, got {labels:?}",
    );
    assert_eq!(labels, vec!["openai"]);
}

#[test]
fn unknown_label_is_silently_dropped() {
    let cfg = VoiceConfig {
        tts_fallback_chain: vec!["nonexistent_provider".into(), "openai".into()],
        ..voicebox_primary_config()
    };
    let chain = resolve_tts_fallback_chain(&cfg, TtsProviderKind::Voicebox);
    let labels: Vec<String> = chain.iter().map(|k| k.label()).collect();
    assert_eq!(labels, vec!["openai"]);
}

#[test]
fn label_aliases_resolve_correctly() {
    assert_eq!(
        TtsProviderKind::from_label("openai-compatible").map(|k| k.label()),
        Some("openai_compatible".into()),
    );
    assert_eq!(
        TtsProviderKind::from_label("piper").map(|k| k.label()),
        Some("local".into()),
    );
    assert_eq!(
        TtsProviderKind::from_label("LOCAL_PIPER").map(|k| k.label()),
        Some("local".into()),
    );
    assert_eq!(
        TtsProviderKind::from_label("groq").map(|k| k.label()),
        None,
        "groq is STT-only — TTS chain must reject it",
    );
}
