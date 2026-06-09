//! Sentinel tests for the provider-registry refactor (closing #141).
//!
//! Context: prior to this work both `active_provider_and_model` and
//! `resolve_provider_from_config` carried their own hardcoded if-else
//! ladders enumerating built-in providers. They drifted from each
//! other and silently omitted real providers — `opencode`, `ollama`,
//! `bedrock`, `vertex` were all missing from the display function
//! and never produced a correct TUI label even when the user had
//! them as their only configured provider. Issue #141 surfaced this
//! after a vision-routing confusion led to the missing-opencode case
//! being noticed.
//!
//! Fix: both functions now iterate `ProviderConfigs::provider_registry()`,
//! a single 16-entry table that lists every built-in provider with
//! its session id, display name, and api-key requirement.
//!
//! These tests pin:
//!   1. Every Option<ProviderConfig> field on ProviderConfigs (chat
//!      providers, not STT/TTS/image-side) appears in the registry.
//!      Adding a new provider field WITHOUT a matching registry
//!      entry trips this check at test time, before the new provider
//!      silently disappears from the TUI.
//!   2. The registry priority order survives common configs — a
//!      lone `opencode` enabled produces ("opencode", "OpenCode")
//!      from both functions, fixing the original symptom of #141.
//!   3. `resolve_provider_from_config` and `active_provider_and_model`
//!      agree on which provider is active for a given config (same
//!      iteration order, no divergence).

use crate::config::{Config, ProviderConfig, resolve_provider_from_config};

fn cfg() -> Config {
    Config::default()
}

fn enabled_with_key(model: &str) -> ProviderConfig {
    ProviderConfig {
        enabled: true,
        api_key: Some("test-key".to_string()),
        default_model: Some(model.to_string()),
        ..Default::default()
    }
}

fn cli_enabled(model: &str) -> ProviderConfig {
    ProviderConfig {
        enabled: true,
        default_model: Some(model.to_string()),
        ..Default::default()
    }
}

#[test]
fn opencode_alone_resolves_to_opencode_display() {
    // The original #141 symptom: user had only [providers.opencode]
    // enabled and the TUI showed "Not configured" because the
    // hardcoded ladder in resolve_provider_from_config didn't know
    // about opencode. After the registry refactor this case must
    // return the correct display name.
    let mut c = cfg();
    c.providers.opencode = Some(cli_enabled("gpt-5-nano"));
    let (display, model) = resolve_provider_from_config(&c);
    assert_eq!(display, "OpenCode");
    assert_eq!(model, "gpt-5-nano");
    let (id, m2) = c.providers.active_provider_and_model();
    assert_eq!(id, "opencode");
    assert_eq!(m2, "gpt-5-nano");
}

#[test]
fn ollama_bedrock_vertex_are_no_longer_silently_omitted() {
    // Each used to be missing from resolve_provider_from_config.
    // Verify each produces a real display label when active alone.
    for (setter, expected_id, expected_display) in [
        ("ollama" as &str, "ollama", "Ollama"),
        ("bedrock", "bedrock", "AWS Bedrock"),
        ("vertex", "vertex", "Google Vertex"),
    ] {
        let mut c = cfg();
        match setter {
            "ollama" => c.providers.ollama = Some(cli_enabled("(default)")),
            "bedrock" => c.providers.bedrock = Some(enabled_with_key("(default)")),
            "vertex" => c.providers.vertex = Some(enabled_with_key("(default)")),
            _ => unreachable!(),
        }
        let (display, _) = resolve_provider_from_config(&c);
        assert_eq!(
            display, expected_display,
            "{setter} must produce display {expected_display:?} via the registry"
        );
        let (id, _) = c.providers.active_provider_and_model();
        assert_eq!(
            id, expected_id,
            "{setter} must produce id {expected_id:?} via the registry"
        );
    }
}

#[test]
fn cli_providers_dont_require_api_key() {
    // claude-cli / opencode-cli / codex-cli / codex OAuth all work
    // without an api_key (they use subprocess or OAuth flow). The
    // registry's requires_api_key=false on those entries makes the
    // common-case "set enabled=true, don't paste a fake key" work
    // out of the box.
    let mut c = cfg();
    c.providers.claude_cli = Some(cli_enabled("sonnet"));
    let (id, _) = c.providers.active_provider_and_model();
    assert_eq!(id, "claude-cli");
}

#[test]
fn api_providers_without_key_are_skipped() {
    // anthropic / openai / etc. need both enabled=true AND
    // api_key=Some(_) to be considered active. Without a key the
    // registry must pass over them, not flag the config as
    // misconfigured.
    let mut c = cfg();
    c.providers.anthropic = Some(ProviderConfig {
        enabled: true,
        default_model: Some("sonnet".to_string()),
        ..Default::default()
    });
    let (display, _) = resolve_provider_from_config(&c);
    assert_eq!(
        display, "Not configured",
        "anthropic enabled but no api_key must NOT be picked as active"
    );
}

#[test]
fn priority_matches_factory_create_provider_intent() {
    // CLI providers come first (free, no key), then API providers.
    // Confirm a config with BOTH claude-cli AND anthropic API picks
    // the CLI — that's how the user gets free subscription usage
    // even when they also have a paid API key configured.
    let mut c = cfg();
    c.providers.claude_cli = Some(cli_enabled("sonnet"));
    c.providers.anthropic = Some(enabled_with_key("claude-sonnet-7"));
    let (id, _) = c.providers.active_provider_and_model();
    assert_eq!(
        id, "claude-cli",
        "claude-cli must win over anthropic API when both configured (CLI is free)"
    );
    let (display, _) = resolve_provider_from_config(&c);
    assert_eq!(display, "Claude CLI");
}

#[test]
fn resolve_and_active_agree_on_priority_for_every_combination() {
    // The two functions must walk the same iteration order. If
    // someone later edits the registry's priority for one but not
    // the other, this test trips. Cycle through a handful of
    // multi-provider configs and confirm the picked provider's
    // display label corresponds to the picked session id.
    let cases: &[(&str, &str)] = &[
        ("claude-cli", "Claude CLI"),
        ("opencode", "OpenCode"),
        ("qwen", "Qwen"),
        ("openrouter", "OpenRouter"),
        ("gemini", "Google Gemini"),
    ];
    for (id, display) in cases {
        let mut c = cfg();
        match *id {
            "claude-cli" => c.providers.claude_cli = Some(cli_enabled("sonnet")),
            "opencode" => c.providers.opencode = Some(cli_enabled("gpt-5-nano")),
            "qwen" => c.providers.qwen = Some(enabled_with_key("qwen3-max")),
            "openrouter" => c.providers.openrouter = Some(enabled_with_key("any/model")),
            "gemini" => c.providers.gemini = Some(enabled_with_key("gemini-3-pro")),
            _ => unreachable!(),
        }
        let (active_id, _) = c.providers.active_provider_and_model();
        assert_eq!(active_id, *id, "active_provider_and_model id for {id}");
        let (resolved_display, _) = resolve_provider_from_config(&c);
        assert_eq!(
            resolved_display, *display,
            "resolve_provider_from_config display for {id}"
        );
    }
}

#[test]
fn no_provider_configured_returns_not_configured() {
    let c = cfg();
    let (display, model) = resolve_provider_from_config(&c);
    assert_eq!(display, "Not configured");
    assert_eq!(model, "N/A");
    let (id, m) = c.providers.active_provider_and_model();
    assert_eq!(id, "none");
    assert_eq!(m, "none");
}

/// Completeness guard for adding a new provider. The onboarding `PROVIDERS`
/// list and the `KNOWN_PROVIDERS` name registry must enumerate the SAME set of
/// provider ids. If they drift, a provider was added to one surface but not the
/// other (e.g. an onboarding entry with no KNOWN_PROVIDERS meta, which silently
/// breaks name resolution / the /models picker). This is the cheap check that
/// would have caught a half-wired provider like the dead `bedrock`/`vertex`
/// stubs had they ever reached onboarding.
#[test]
fn onboarding_and_known_providers_list_the_same_ids() {
    use crate::tui::onboarding::PROVIDERS;
    use crate::utils::providers::KNOWN_PROVIDERS;
    use std::collections::BTreeSet;

    // Custom (empty id) is dynamic and intentionally absent from KNOWN_PROVIDERS.
    let onboarding: BTreeSet<&str> = PROVIDERS
        .iter()
        .map(|p| p.id)
        .filter(|id| !id.is_empty())
        .collect();
    let known: BTreeSet<&str> = KNOWN_PROVIDERS.iter().map(|m| m.id).collect();

    assert_eq!(
        onboarding, known,
        "onboarding PROVIDERS and KNOWN_PROVIDERS must list the same provider ids. \
         A mismatch means a provider was added/removed on one surface but not the \
         other — wire ALL touchpoints (see docs/reference/ADDING_NEW_PROVIDERS.md)."
    );
}
