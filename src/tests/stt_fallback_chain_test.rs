//! STT fallback chain dispatcher tests.
//!
//! Exercises `voice::service::resolve_fallback_chain` to confirm the
//! user-configured `stt_fallback_chain` produces the right sequence of
//! provider attempts when the primary is failing.

use crate::channels::voice::service::resolve_fallback_chain;
use crate::channels::voice::service::SttProviderKind;
use crate::config::{ProviderConfig, VoiceConfig};

fn voicebox_primary_config() -> VoiceConfig {
    VoiceConfig {
        voicebox_stt_enabled: true,
        voicebox_stt_base_url: "http://localhost:8000".to_string(),
        stt_base_url: Some("https://api.openai.com/v1/audio/transcriptions".to_string()),
        stt_model: Some("whisper-1".to_string()),
        stt_api_key: Some("sk-test".to_string()),
        stt_provider: Some(ProviderConfig {
            api_key: Some("groq-key".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn empty_chain_uses_default_priority_with_primary_skipped() {
    let cfg = voicebox_primary_config();
    let chain = resolve_fallback_chain(&cfg, SttProviderKind::Voicebox);
    let labels: Vec<String> = chain.iter().map(|k| k.label()).collect();
    // Voicebox is primary → excluded; default order is the remaining
    // three in declaration order, filtered by what's configured.
    // Local is included on builds with the `local-stt` feature, skipped
    // otherwise.
    assert!(
        labels.contains(&"openai_compatible".to_string()),
        "default chain should include openai_compatible when configured, got {labels:?}",
    );
    assert!(
        labels.contains(&"groq".to_string()),
        "default chain should include groq when configured, got {labels:?}",
    );
    assert!(
        !labels.contains(&"voicebox".to_string()),
        "primary must be excluded from its own fallback chain, got {labels:?}",
    );
}

#[test]
fn user_chain_order_is_respected() {
    let cfg = VoiceConfig {
        stt_fallback_chain: vec!["groq".into(), "openai_compatible".into()],
        ..voicebox_primary_config()
    };
    let chain = resolve_fallback_chain(&cfg, SttProviderKind::Voicebox);
    let labels: Vec<String> = chain.iter().map(|k| k.label()).collect();
    assert_eq!(labels, vec!["groq", "openai_compatible"]);
}

#[test]
fn unconfigured_entries_are_filtered_out() {
    // Chain lists three providers, but only voicebox has any config —
    // and voicebox IS the primary so it's also excluded. The chain
    // should be empty.
    let cfg = VoiceConfig {
        voicebox_stt_enabled: true,
        voicebox_stt_base_url: "http://localhost:8000".to_string(),
        stt_fallback_chain: vec![
            "voicebox".into(),
            "openai_compatible".into(),
            "groq".into(),
        ],
        ..Default::default()
    };
    let chain = resolve_fallback_chain(&cfg, SttProviderKind::Voicebox);
    assert!(chain.is_empty(), "unconfigured providers must be skipped");
}

#[test]
fn primary_is_never_re_attempted_via_chain() {
    let cfg = VoiceConfig {
        stt_fallback_chain: vec!["voicebox".into(), "voicebox".into(), "groq".into()],
        ..voicebox_primary_config()
    };
    let chain = resolve_fallback_chain(&cfg, SttProviderKind::Voicebox);
    let labels: Vec<String> = chain.iter().map(|k| k.label()).collect();
    assert!(
        !labels.contains(&"voicebox".to_string()),
        "primary must be skipped even when the user lists it explicitly, got {labels:?}",
    );
    assert_eq!(labels, vec!["groq"]);
}

#[test]
fn unknown_label_is_silently_dropped() {
    let cfg = VoiceConfig {
        stt_fallback_chain: vec!["nonexistent_provider".into(), "groq".into()],
        ..voicebox_primary_config()
    };
    let chain = resolve_fallback_chain(&cfg, SttProviderKind::Voicebox);
    let labels: Vec<String> = chain.iter().map(|k| k.label()).collect();
    assert_eq!(labels, vec!["groq"]);
}

#[test]
fn label_aliases_resolve_correctly() {
    assert_eq!(
        SttProviderKind::from_label("openai-compatible").map(|k| k.label()),
        Some("openai_compatible".into()),
    );
    assert_eq!(
        SttProviderKind::from_label("Local Whisper").map(|k| k.label()),
        None,
        "spaces are part of the lookup — won't resolve",
    );
    assert_eq!(
        SttProviderKind::from_label("local_whisper").map(|k| k.label()),
        Some("local".into()),
    );
    assert_eq!(
        SttProviderKind::from_label("WHISPER").map(|k| k.label()),
        Some("local".into()),
    );
}
