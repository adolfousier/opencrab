//! Regression tests for `merge_provider_keys`.
//!
//! Background: a key written to `keys.toml` under
//! `[providers.<name>] api_key = "…"` only takes effect at runtime if
//! `merge_provider_keys` has an explicit branch for `<name>`. Adding a
//! new top-level provider field on `ProviderConfigs` without adding a
//! corresponding merge branch causes a silent failure: the key is on
//! disk, the running config never sees it, and the provider factory
//! reports "API key missing" with no obvious cause.
//!
//! These tests pin the contract for the providers we ship today.

use crate::config::{
    OpenaiCompatibleSttConfig, OpenaiCompatibleTtsConfig, ProviderConfig, ProviderConfigs,
    SttProviders, TtsProviders, merge_provider_keys,
};

fn key_only(api_key: &str) -> ProviderConfig {
    ProviderConfig {
        api_key: Some(api_key.to_string()),
        ..Default::default()
    }
}

#[test]
fn opencode_api_key_from_keys_toml_lands_in_runtime_config() {
    // Repro for the v0.3.16 bug: /models writes `[providers.opencode]
    // api_key = "..."` to keys.toml, but on the next config reload
    // merge_provider_keys was missing an opencode branch — runtime
    // Config.providers.opencode.api_key stayed None, factory.rs
    // reported "API key missing", and the new selection silently
    // failed to take effect.
    let base = ProviderConfigs::default();
    let keys = ProviderConfigs {
        opencode: Some(key_only("oc_test_key")),
        ..Default::default()
    };

    let merged = merge_provider_keys(base, keys);
    let opencode = merged.opencode.expect("opencode entry created");
    assert_eq!(opencode.api_key.as_deref(), Some("oc_test_key"));
    assert!(
        opencode.enabled,
        "first-time keys.toml load should auto-enable opencode"
    );
}

#[test]
fn opencode_existing_config_disabled_state_is_preserved_on_key_merge() {
    // If config.toml has `enabled = false` for opencode but keys.toml
    // carries an api_key, the user's explicit disabled state wins —
    // we only auto-enable when there's no entry at all.
    let base = ProviderConfigs {
        opencode: Some(ProviderConfig {
            enabled: false,
            ..Default::default()
        }),
        ..Default::default()
    };
    let keys = ProviderConfigs {
        opencode: Some(key_only("oc_test_key")),
        ..Default::default()
    };

    let merged = merge_provider_keys(base, keys);
    let opencode = merged.opencode.expect("opencode entry preserved");
    assert_eq!(opencode.api_key.as_deref(), Some("oc_test_key"));
    assert!(
        !opencode.enabled,
        "user's explicit disabled state must not flip on key merge"
    );
}

#[test]
fn sentinel_placeholder_does_not_leak_into_runtime_config() {
    // /models uses `__EXISTING_KEY__` internally to mean "keep the
    // current key". The merge function must never propagate that
    // sentinel into the runtime config.
    let base = ProviderConfigs::default();
    let keys = ProviderConfigs {
        opencode: Some(key_only("__EXISTING_KEY__")),
        ..Default::default()
    };

    let merged = merge_provider_keys(base, keys);
    assert!(
        merged.opencode.is_none(),
        "sentinel must not create an opencode entry"
    );
}

#[test]
fn anthropic_openai_qwen_keys_still_merge_after_opencode_addition() {
    // Smoke test that the existing branches still work — protects
    // against accidental regressions when adding new branches.
    let base = ProviderConfigs::default();
    let keys = ProviderConfigs {
        anthropic: Some(key_only("ant_key")),
        openai: Some(key_only("oai_key")),
        qwen: Some(key_only("qwen_key")),
        ..Default::default()
    };

    let merged = merge_provider_keys(base, keys);
    assert_eq!(
        merged.anthropic.and_then(|c| c.api_key).as_deref(),
        Some("ant_key")
    );
    assert_eq!(
        merged.openai.and_then(|c| c.api_key).as_deref(),
        Some("oai_key")
    );
    assert_eq!(
        merged.qwen.and_then(|c| c.api_key).as_deref(),
        Some("qwen_key")
    );
}

#[test]
fn moonshot_api_key_from_keys_toml_lands_in_runtime_config() {
    // #610: the built-in Moonshot AI (Kimi) provider wrote its key to keys.toml
    // ("Wrote secret key [providers.moonshot].api_key") but merge_provider_keys
    // had no moonshot branch, so runtime config.providers.moonshot.api_key
    // stayed None and the factory reported "Moonshot AI enabled but API key
    // missing" right after setup.
    let base = ProviderConfigs::default();
    let keys = ProviderConfigs {
        moonshot: Some(key_only("sk-kimi-test")),
        ..Default::default()
    };
    let merged = merge_provider_keys(base, keys);
    let moonshot = merged.moonshot.expect("moonshot entry created");
    assert_eq!(moonshot.api_key.as_deref(), Some("sk-kimi-test"));
}

#[test]
fn ollama_api_key_from_keys_toml_lands_in_runtime_config() {
    // #1066: ProviderConfigs has carried an `ollama` field for a while, so a
    // cloud key under [providers.ollama] in keys.toml deserialised cleanly and
    // was then dropped for want of a merge arm. The factory reads the key with
    // unwrap_or_default(), so the drop became an empty bearer token and
    // api.ollama.com answered 401. Local Ollama needs no key and never noticed.
    let base = ProviderConfigs::default();
    let keys = ProviderConfigs {
        ollama: Some(key_only("ollama_cloud_key")),
        ..Default::default()
    };
    let merged = merge_provider_keys(base, keys);
    let ollama = merged.ollama.expect("ollama entry created");
    assert_eq!(ollama.api_key.as_deref(), Some("ollama_cloud_key"));
}

#[test]
fn stt_openai_compatible_api_key_lands_in_runtime_config() {
    // #1066: /onboard writes this key to keys.toml, but merge_provider_keys had
    // an arm only for stt.groq — so the key the supported setup flow writes was
    // exactly the one that never reached the runtime.
    let base = ProviderConfigs::default();
    let keys = ProviderConfigs {
        stt: Some(SttProviders {
            openai_compatible: Some(OpenaiCompatibleSttConfig {
                api_key: Some("stt_compat_key".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let merged = merge_provider_keys(base, keys);
    assert_eq!(
        merged
            .stt
            .and_then(|s| s.openai_compatible)
            .and_then(|c| c.api_key)
            .as_deref(),
        Some("stt_compat_key")
    );
}

#[test]
fn tts_openai_compatible_api_key_lands_in_runtime_config() {
    // #1066: mirror of the STT gap — [providers.tts.openai_compatible] parsed
    // fine and was discarded, so a configured OpenAI-compatible voice endpoint
    // spoke with no credential.
    let base = ProviderConfigs::default();
    let keys = ProviderConfigs {
        tts: Some(TtsProviders {
            openai_compatible: Some(OpenaiCompatibleTtsConfig {
                api_key: Some("tts_compat_key".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let merged = merge_provider_keys(base, keys);
    assert_eq!(
        merged
            .tts
            .and_then(|s| s.openai_compatible)
            .and_then(|c| c.api_key)
            .as_deref(),
        Some("tts_compat_key")
    );
}

#[test]
fn both_keyed_voice_providers_survive_one_merge() {
    // Restructuring the STT/TTS arms to cover openai_compatible must not cost
    // groq/openai their keys: keys.stt is moved by the outer `if let`, so the
    // two providers have to be handled inside a single block.
    let base = ProviderConfigs::default();
    let keys = ProviderConfigs {
        stt: Some(SttProviders {
            groq: Some(key_only("groq_key")),
            openai_compatible: Some(OpenaiCompatibleSttConfig {
                api_key: Some("stt_compat_key".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        }),
        tts: Some(TtsProviders {
            openai: Some(key_only("tts_openai_key")),
            openai_compatible: Some(OpenaiCompatibleTtsConfig {
                api_key: Some("tts_compat_key".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let merged = merge_provider_keys(base, keys);
    let stt = merged.stt.expect("stt section created");
    let tts = merged.tts.expect("tts section created");
    assert_eq!(
        stt.groq.and_then(|c| c.api_key).as_deref(),
        Some("groq_key")
    );
    assert_eq!(
        stt.openai_compatible.and_then(|c| c.api_key).as_deref(),
        Some("stt_compat_key")
    );
    assert_eq!(
        tts.openai.and_then(|c| c.api_key).as_deref(),
        Some("tts_openai_key")
    );
    assert_eq!(
        tts.openai_compatible.and_then(|c| c.api_key).as_deref(),
        Some("tts_compat_key")
    );
}

#[test]
fn voice_sentinel_placeholder_is_never_merged() {
    // `__EXISTING_KEY__` is what /models writes internally to mean "keep the
    // stored key". The STT/TTS arms had no is_real_key guard, so the sentinel
    // could land in runtime config as though it were a credential (#1066).
    let base = ProviderConfigs::default();
    let keys = ProviderConfigs {
        stt: Some(SttProviders {
            groq: Some(key_only("__EXISTING_KEY__")),
            ..Default::default()
        }),
        tts: Some(TtsProviders {
            openai: Some(key_only("__EXISTING_KEY__")),
            ..Default::default()
        }),
        ..Default::default()
    };
    let merged = merge_provider_keys(base, keys);
    assert!(
        merged.stt.is_none(),
        "sentinel must not create an stt section"
    );
    assert!(
        merged.tts.is_none(),
        "sentinel must not create a tts section"
    );
}

#[test]
fn a_key_stored_with_the_sentinel_glued_to_it_still_merges() {
    // An older build let a seeded field that was typed into persist as
    // `__EXISTING_KEY__<key>`, so keys.toml on real machines already holds that
    // shape. The merge sanitises rather than rejects, which is what heals those
    // machines on the next load instead of asking for a hand edit (#1075).
    let base = ProviderConfigs::default();
    let keys = ProviderConfigs {
        anthropic: Some(key_only("__EXISTING_KEY__sk-ant-real")),
        tts: Some(TtsProviders {
            openai: Some(key_only("__EXISTING_KEY__sk-tts-real")),
            ..Default::default()
        }),
        ..Default::default()
    };
    let merged = merge_provider_keys(base, keys);
    assert_eq!(
        merged.anthropic.and_then(|c| c.api_key).as_deref(),
        Some("sk-ant-real"),
        "the marker must be stripped, not treated as part of the key"
    );
    assert_eq!(
        merged
            .tts
            .and_then(|t| t.openai)
            .and_then(|c| c.api_key)
            .as_deref(),
        Some("sk-tts-real")
    );
}

#[test]
fn empty_voice_sections_do_not_create_phantom_config() {
    // A [providers.stt] table carrying no real key must not materialise a
    // base.stt section. Phantom entries are how the resurrected-provider class
    // of bug starts, so the arm only allocates once a real key has arrived.
    let base = ProviderConfigs::default();
    let keys = ProviderConfigs {
        stt: Some(SttProviders::default()),
        tts: Some(TtsProviders::default()),
        ..Default::default()
    };
    let merged = merge_provider_keys(base, keys);
    assert!(merged.stt.is_none(), "empty stt keys must not allocate");
    assert!(merged.tts.is_none(), "empty tts keys must not allocate");
}
