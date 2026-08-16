//! The memory embedding key is resolved from keys.toml (#1066).
//!
//! `EmbeddingConfig::api_key` has always documented that it is "also loaded
//! from keys.toml under `[providers.memory_embedding]`", and nothing
//! implemented it. The gap was two layers deep: `ProviderConfigs` has no
//! `memory_embedding` field so serde discarded the section outright, and even
//! a merge arm would not have helped, because `src/memory/mod.rs` parses
//! config.toml directly and never passes through `Config::load()`.
//!
//! The symptom was silent: the embedding call 401s and memory degrades to
//! keyword-only FTS with no error surfaced.
//!
//! Fixtures are synthetic and carry no real credentials.

use crate::memory::{memory_embedding_key_in, needs_embedding_key};

#[test]
fn a_key_under_the_documented_section_is_found() {
    let keys = r#"
[providers.anthropic]
api_key = "sk-ant-unrelated"

[providers.memory_embedding]
api_key = "sk-embed-test"
"#;
    assert_eq!(
        memory_embedding_key_in(keys).as_deref(),
        Some("sk-embed-test")
    );
}

#[test]
fn the_models_sentinel_is_not_a_credential() {
    // `__EXISTING_KEY__` is what /models writes internally to mean "keep the
    // stored key". Merging it would send the literal string as a bearer token,
    // which fails in a way that looks exactly like a bad key.
    let keys = r#"
[providers.memory_embedding]
api_key = "__EXISTING_KEY__"
"#;
    assert!(memory_embedding_key_in(keys).is_none());
}

#[test]
fn a_blank_or_missing_section_yields_nothing() {
    for keys in [
        "",
        "[providers.memory_embedding]\napi_key = \"\"\n",
        "[providers.memory_embedding]\napi_key = \"   \"\n",
        "[providers.openai]\napi_key = \"sk-other\"\n",
        "[providers.memory_embedding]\nbase_url = \"https://example.invalid\"\n",
    ] {
        assert!(
            memory_embedding_key_in(keys).is_none(),
            "must not invent a key from: {keys:?}"
        );
    }
}

#[test]
fn surrounding_whitespace_is_trimmed_off_the_key() {
    // A key pasted with a trailing newline is a real shape, and an untrimmed
    // bearer token is rejected by most gateways.
    let keys = "[providers.memory_embedding]\napi_key = \"  sk-embed-padded \"\n";
    assert_eq!(
        memory_embedding_key_in(keys).as_deref(),
        Some("sk-embed-padded")
    );
}

#[test]
fn malformed_keys_toml_does_not_panic() {
    assert!(memory_embedding_key_in("this is not = = toml [[[").is_none());
}

#[test]
fn config_toml_wins_when_it_carries_a_real_key() {
    // The keys.toml fallback only fills a hole. An explicit key in
    // [memory.embedding] must keep priority, or the documented workaround for
    // this very bug would stop working.
    assert!(!needs_embedding_key(Some("sk-from-config")));
}

#[test]
fn an_absent_or_blank_config_key_defers_to_keys_toml() {
    assert!(needs_embedding_key(None));
    assert!(needs_embedding_key(Some("")));
    assert!(needs_embedding_key(Some("   ")));
}
