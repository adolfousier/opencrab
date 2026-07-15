//! Regression tests for provider config section resolution.
//!
//! These tests ensure that adding or reordering providers can NEVER corrupt
//! config.toml by writing to the wrong section again (2026-04-28 bug where
//! custom provider selection wrote to [providers.anthropic]).
//!
//! Coverage:
//! - Every KNOWN_PROVIDER maps to the correct TOML section
//! - TUI PROVIDERS are in sync with KNOWN_PROVIDERS
//! - config_section() returns correct values for built-ins and customs
//! - is_first_time() checks all providers including new ones

use crate::utils::providers::{
    KNOWN_PROVIDERS, config_section, find_provider_meta, normalize_provider_name,
};

// ── KNOWN_PROVIDERS section mapping ─────────────────────────────────

#[test]
fn known_provider_anthropic_section() {
    let meta = find_provider_meta("anthropic").expect("anthropic must exist");
    assert_eq!(meta.config_section, "providers.anthropic");
    assert_eq!(meta.id, "anthropic");
    assert!(meta.needs_api_key);
}

#[test]
fn known_provider_openai_section() {
    let meta = find_provider_meta("openai").expect("openai must exist");
    assert_eq!(meta.config_section, "providers.openai");
}

#[test]
fn known_provider_github_section() {
    let meta = find_provider_meta("github").expect("github must exist");
    assert_eq!(meta.config_section, "providers.github");
}

#[test]
fn known_provider_gemini_section() {
    let meta = find_provider_meta("gemini").expect("gemini must exist");
    assert_eq!(meta.config_section, "providers.gemini");
}

#[test]
fn known_provider_openrouter_section() {
    let meta = find_provider_meta("openrouter").expect("openrouter must exist");
    assert_eq!(meta.config_section, "providers.openrouter");
}

#[test]
fn known_provider_minimax_section() {
    let meta = find_provider_meta("minimax").expect("minimax must exist");
    assert_eq!(meta.config_section, "providers.minimax");
}

#[test]
fn known_provider_zhipu_section() {
    let meta = find_provider_meta("zhipu").expect("zhipu must exist");
    assert_eq!(meta.config_section, "providers.zhipu");
}

#[test]
fn known_provider_claude_cli_section() {
    let meta = find_provider_meta("claude_cli").expect("claude_cli must exist");
    assert_eq!(meta.config_section, "providers.claude_cli");
    assert!(!meta.needs_api_key);
}

#[test]
fn known_provider_opencode_cli_section() {
    let meta = find_provider_meta("opencode_cli").expect("opencode_cli must exist");
    assert_eq!(meta.config_section, "providers.opencode_cli");
    assert!(!meta.needs_api_key);
}

#[test]
fn known_provider_codex_cli_section() {
    let meta = find_provider_meta("codex_cli").expect("codex_cli must exist");
    assert_eq!(meta.config_section, "providers.codex_cli");
    assert!(!meta.needs_api_key);
}

#[test]
fn known_provider_codex_oauth_section() {
    let meta = find_provider_meta("codex").expect("codex must exist");
    assert_eq!(meta.config_section, "providers.codex");
    assert!(!meta.needs_api_key);
}

#[test]
fn known_provider_opencode_section() {
    let meta = find_provider_meta("opencode").expect("opencode must exist");
    assert_eq!(meta.config_section, "providers.opencode");
    assert!(meta.needs_api_key);
}

#[test]
fn known_provider_qwen_section() {
    let meta = find_provider_meta("qwen").expect("qwen must exist");
    assert_eq!(meta.config_section, "providers.qwen");
}

#[test]
fn known_provider_ollama_section() {
    let meta = find_provider_meta("ollama").expect("ollama must exist");
    assert_eq!(meta.config_section, "providers.ollama");
}

// ── config_section() function ─────────────────��─────────────────────

#[test]
fn config_section_builtin_returns_correct() {
    assert_eq!(
        config_section("anthropic"),
        Some("providers.anthropic".to_string())
    );
    assert_eq!(
        config_section("openai"),
        Some("providers.openai".to_string())
    );
    assert_eq!(
        config_section("ollama"),
        Some("providers.ollama".to_string())
    );
    assert_eq!(
        config_section("opencode"),
        Some("providers.opencode".to_string())
    );
}

#[test]
fn config_section_custom_returns_correct() {
    assert_eq!(
        config_section("custom:dialagram"),
        Some("providers.custom.dialagram".to_string())
    );
    assert_eq!(
        config_section("custom:opencode-qwen"),
        Some("providers.custom.opencode-qwen".to_string())
    );
    assert_eq!(
        config_section("custom(opencode-kimi)"),
        Some("providers.custom.opencode-kimi".to_string())
    );
}

#[test]
fn config_section_unknown_returns_none() {
    assert_eq!(config_section("nonexistent"), None);
}

// ── normalize_provider_name ─────────────────────────────────────────

#[test]
fn normalize_builtin_returns_canonical_id() {
    assert_eq!(normalize_provider_name("Anthropic"), "anthropic");
    assert_eq!(normalize_provider_name("claude_cli"), "claude-cli");
    assert_eq!(normalize_provider_name("opencode_cli"), "opencode-cli");
    assert_eq!(normalize_provider_name("codex_cli"), "codex-cli");
    assert_eq!(normalize_provider_name("codex_oauth"), "codex");
    assert_eq!(normalize_provider_name("Codex CLI"), "codex-cli");
    assert_eq!(normalize_provider_name("Codex"), "codex");
    assert_eq!(normalize_provider_name("Ollama"), "ollama");
}

#[test]
fn normalize_custom_preserves_prefix() {
    assert_eq!(
        normalize_provider_name("custom:dialagram"),
        "custom:dialagram"
    );
    assert_eq!(
        normalize_provider_name("Custom(dialagram)"),
        "custom:dialagram"
    );
}

// ── KNOWN_PROVIDERS completeness ────────────────────────────────────

#[test]
fn all_known_providers_have_unique_ids() {
    let ids: Vec<&str> = KNOWN_PROVIDERS.iter().map(|p| p.id).collect();
    let mut seen = std::collections::HashSet::new();
    for id in &ids {
        assert!(seen.insert(*id), "Duplicate provider id: {}", id);
    }
}

#[test]
fn all_known_providers_have_unique_sections() {
    let sections: Vec<&str> = KNOWN_PROVIDERS.iter().map(|p| p.config_section).collect();
    let mut seen = std::collections::HashSet::new();
    for s in &sections {
        assert!(seen.insert(*s), "Duplicate config section: {}", s);
    }
}

#[test]
fn all_known_providers_have_non_empty_display_names() {
    for p in KNOWN_PROVIDERS {
        assert!(
            !p.display_name.is_empty(),
            "Provider {} has empty display_name",
            p.id
        );
    }
}

#[test]
fn known_provider_count_matches_expected() {
    // If this fails, a provider was added/removed.
    // Update this count AND verify all section mappings above.
    assert_eq!(KNOWN_PROVIDERS.len(), 16);
}

// ── TUI PROVIDERS sync with KNOWN_PROVIDERS ─────────────────────────

#[test]
fn tui_providers_all_have_matching_known_provider() {
    use crate::tui::onboarding::PROVIDERS;

    for p in PROVIDERS {
        // Custom provider has empty id — skip it
        if p.id.is_empty() {
            continue;
        }
        let meta = find_provider_meta(p.id);
        assert!(
            meta.is_some(),
            "TUI provider '{}' (id='{}') has no matching KNOWN_PROVIDER entry",
            p.name,
            p.id
        );
        let meta = meta.unwrap();
        assert_eq!(
            meta.config_section,
            format!("providers.{}", p.id.replace('-', "_")),
            "TUI provider '{}' section mismatch",
            p.id
        );
    }
}

#[test]
fn tui_custom_provider_is_last() {
    use crate::tui::onboarding::PROVIDERS;

    let last = PROVIDERS.last().expect("PROVIDERS must not be empty");
    assert!(
        last.id.is_empty(),
        "Last PROVIDERS entry must be Custom (empty id), got '{}'",
        last.id
    );
    assert!(
        last.name.contains("Custom"),
        "Last PROVIDERS entry must be named Custom, got '{}'",
        last.name
    );
}

// ── is_first_time provider coverage ─────────────────────────────────

#[test]
fn is_first_time_uses_provider_registry_not_hardcoded_list() {
    // is_first_time() must derive "is there a configured provider?" from
    // active_provider_and_model() (which walks provider_registry — the single
    // source of truth), NOT a hardcoded OR-chain of provider fields.
    //
    // History: the hardcoded chain silently omitted new providers — ollama and
    // opencode (2026-04-28), then Xiaomi (2026-06) — so an enabled but
    // un-listed provider re-triggered onboarding on every restart. Routing
    // through provider_registry makes that class of bug impossible: adding a
    // provider to the registry automatically covers is_first_time().
    let source = include_str!("../tui/onboarding/fetch.rs");
    assert!(
        source.contains("active_provider_and_model"),
        "is_first_time() must use active_provider_and_model (provider_registry), \
         not a hardcoded provider list that drifts every time a provider is added"
    );
}

// ── apply_config section routing (registry-driven) ──────────────────
//
// The save path that /models reuses lives in `OnboardingState::apply_config`.
// The old ModelSelector resolved sections via a hand-maintained `match
// provider.id` with one arm per provider — a list that could silently drift
// out of sync with the providers themselves. The refactor replaced that with
// the `utils::providers` registry: section resolution goes through
// `find_provider_meta(id).config_section` and the disable-all sweep iterates
// `all_config_sections` / `KNOWN_PROVIDERS`. These tests pin the stronger,
// drift-proof guarantee: the registry routes every provider, and the save
// path stays registry-driven (never reverts to a hardcoded match).

const SAVE_SRC: &str = include_str!("../tui/onboarding/config.rs");

#[test]
fn save_provider_section_routing_covers_all_providers() {
    // Every provider id resolves to a TOML section through the registry —
    // this is what prevents the index-based corruption bug (writing a custom
    // provider's settings into [providers.anthropic]).
    let required_ids = [
        "anthropic",
        "openai",
        "github",
        "gemini",
        "openrouter",
        "minimax",
        "zhipu",
        "claude-cli",
        "opencode-cli",
        "codex-cli",
        "codex",
        "opencode",
        "qwen",
        "ollama",
    ];

    for id in &required_ids {
        let meta = find_provider_meta(id).unwrap_or_else(|| {
            panic!("provider '{id}' is missing from the KNOWN_PROVIDERS registry")
        });
        assert!(
            meta.config_section.starts_with("providers."),
            "provider '{id}' resolves to a malformed section '{}'",
            meta.config_section
        );
    }

    // Pin that the save path resolves sections via the registry, not a
    // hardcoded `match provider.id` (the drift-prone shape that was removed).
    assert!(
        SAVE_SRC.contains("find_provider_meta(id)"),
        "apply_config must resolve the provider section via \
         find_provider_meta(id).config_section, not a hardcoded match"
    );
}

#[test]
fn save_provider_disables_all_known_sections() {
    // Every built-in section the disable-all sweep must cover is present in the
    // registry, so iterating KNOWN_PROVIDERS / all_config_sections disables
    // them all without a hand-maintained list.
    let required_sections = [
        "providers.anthropic",
        "providers.openai",
        "providers.github",
        "providers.gemini",
        "providers.openrouter",
        "providers.minimax",
        "providers.zhipu",
        "providers.claude_cli",
        "providers.opencode_cli",
        "providers.codex_cli",
        "providers.codex",
        "providers.opencode",
        "providers.qwen",
        "providers.ollama",
    ];

    for section in &required_sections {
        assert!(
            KNOWN_PROVIDERS.iter().any(|p| p.config_section == *section),
            "section '{section}' is missing from the KNOWN_PROVIDERS registry, so the \
             registry-driven disable-all sweep would skip it"
        );
    }

    // Pin that the disable-all sweep stays registry-driven.
    assert!(
        SAVE_SRC.contains("all_config_sections"),
        "apply_config's disable-all loop must iterate all_config_sections \
         (registry-driven), not a hardcoded section list"
    );
}
