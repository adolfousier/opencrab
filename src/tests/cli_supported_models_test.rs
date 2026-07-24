//! Pin the canonical CLI provider model lists so the `/models` menu in
//! channel handlers can never drift from what the provider actually serves.
//!
//! The 2026-05-27 bug ("OpenCode CLI menu shows Claude model names") happened
//! because `channels::commands::models_for_provider` hardcoded a duplicate
//! list that diverged from `OpenCodeCliProvider::supported_models()`. Now
//! both read from `pub(crate) const SUPPORTED_MODELS` in the provider
//! module, surfaced via `utils::providers::cli_supported_models`. These
//! tests fail loudly if anyone reintroduces a parallel list.

use crate::brain::provider::{
    Provider, claude_cli::ClaudeCliProvider, opencode_cli::OpenCodeCliProvider,
};
use crate::utils::providers::cli_supported_models;

#[test]
fn claude_cli_menu_matches_provider_supported_models() {
    // We don't construct ClaudeCliProvider here because `new()` calls
    // resolve_claude_path which probes the filesystem. The claude-cli list is
    // DISCOVERED from the installed CLI (#753) rather than a const, so the
    // invariant is that the menu and the provider read the same discovery
    // function — not that either matches a frozen list.
    let (menu_models, _) =
        cli_supported_models("claude-cli").expect("claude-cli must be a known CLI provider");
    assert_eq!(
        menu_models,
        crate::brain::provider::claude_cli::available_models(),
        "menu and provider must read from the same discovery function"
    );
    // The built-in const remains a floor: discovery may ADD names (a newly
    // released model) but must never drop a known one.
    for m in crate::brain::provider::claude_cli::SUPPORTED_MODELS {
        assert!(
            menu_models.iter().any(|x| x == m),
            "discovery dropped the built-in model '{m}' — the const must stay a floor"
        );
    }
}

#[test]
fn opencode_cli_menu_matches_provider_supported_models() {
    let (menu_models, _) =
        cli_supported_models("opencode-cli").expect("opencode-cli must be a known CLI provider");
    let provider_models: Vec<String> = crate::brain::provider::opencode_cli::SUPPORTED_MODELS
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        menu_models, provider_models,
        "menu and provider must read from the same SUPPORTED_MODELS const"
    );
}

#[test]
fn command_code_cli_menu_matches_provider_supported_models() {
    let (menu_models, default) = cli_supported_models("command-code-cli")
        .expect("command-code-cli must be a known CLI provider");
    let provider_models: Vec<String> = crate::brain::provider::command_code_cli::SUPPORTED_MODELS
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        menu_models, provider_models,
        "menu and provider must read from the same SUPPORTED_MODELS const"
    );
    // The menu default must be a model the CLI actually accepts (taste-1 was a
    // phantom that failed `command-code -m taste-1` with `unknown model`).
    assert!(
        menu_models.iter().any(|m| m == default),
        "default {default} must appear in the menu list"
    );
    assert!(
        !menu_models.iter().any(|m| m == "taste-1"),
        "taste-1 is not a real Command Code model"
    );
}

#[test]
fn underscore_alias_resolves_same_list() {
    let (hyphen, _) = cli_supported_models("opencode-cli").unwrap();
    let (underscore, _) = cli_supported_models("opencode_cli").unwrap();
    assert_eq!(
        hyphen, underscore,
        "hyphen and underscore aliases must resolve to the same canonical list"
    );

    let (hyphen_c, _) = cli_supported_models("claude-cli").unwrap();
    let (underscore_c, _) = cli_supported_models("claude_cli").unwrap();
    assert_eq!(hyphen_c, underscore_c);

    // Command Code exposes a third alias (`cmd-cli`) alongside the hyphen and
    // underscore forms — all three must resolve to the same canonical list.
    let (hyphen_cc, _) = cli_supported_models("command-code-cli").unwrap();
    let (underscore_cc, _) = cli_supported_models("command_code_cli").unwrap();
    let (short_cc, _) = cli_supported_models("cmd-cli").unwrap();
    assert_eq!(hyphen_cc, underscore_cc);
    assert_eq!(hyphen_cc, short_cc);
}

#[test]
fn unknown_provider_returns_none() {
    assert!(cli_supported_models("openai").is_none());
    assert!(cli_supported_models("qwen").is_none());
    assert!(cli_supported_models("anthropic").is_none());
    assert!(cli_supported_models("").is_none());
}

#[test]
fn opencode_cli_does_not_list_claude_names() {
    // Pin the specific 2026-05-27 regression: OpenCode CLI menu must NOT
    // contain Claude model names like sonnet-4.5 or opus-4.1.
    let (models, default) = cli_supported_models("opencode-cli").unwrap();
    for m in &models {
        assert!(
            !m.contains("sonnet") && !m.contains("opus") && !m.contains("haiku"),
            "OpenCode CLI must not list Claude model '{m}' — regression of 2026-05-27 bug"
        );
    }
    assert!(
        default.starts_with("opencode/"),
        "OpenCode CLI default must be an opencode/* model, got: {default}"
    );
}

#[test]
fn claude_cli_default_is_real_claude_model() {
    let (_, default) = cli_supported_models("claude-cli").unwrap();
    assert!(
        default.starts_with("opus")
            || default.starts_with("sonnet")
            || default.starts_with("haiku"),
        "Claude CLI default must be a real Claude model, got: {default}"
    );
}

// Sanity-check the provider's trait method actually returns the const,
// not some other list. Touch every variant the provider could synthesize.

#[test]
fn claude_cli_trait_supported_models_uses_discovery() {
    if let Ok(p) = ClaudeCliProvider::new() {
        let trait_models = p.supported_models();
        // Discovered (#753), so it may be a SUPERSET of the const — the
        // invariant is that it comes from the same discovery the menu uses and
        // never drops a built-in name. Matching the const exactly would defeat
        // the purpose: a newly released model must be allowed through here or
        // picking it would be rejected as "not in the catalogue".
        assert_eq!(
            trait_models,
            crate::brain::provider::claude_cli::available_models()
        );
        for m in crate::brain::provider::claude_cli::SUPPORTED_MODELS {
            assert!(
                trait_models.iter().any(|x| x == m),
                "provider dropped the built-in model '{m}'"
            );
        }
    }
    // If `claude` binary isn't installed in this test env, skip silently;
    // the menu helper test above already pins the invariant.
}

#[test]
fn opencode_cli_trait_supported_models_uses_const() {
    if let Ok(p) = OpenCodeCliProvider::new() {
        let trait_models = p.supported_models();
        let const_models: Vec<String> = crate::brain::provider::opencode_cli::SUPPORTED_MODELS
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(trait_models, const_models);
    }
}

// ── --help discovery parser (#753) ──────────────────────────────

/// The real shape of `claude --help` for the `--model` option, including the
/// wrapped continuation lines and the apostrophe in "model's" that breaks
/// naive quote pairing.
const REAL_HELP: &str = r#"Options:
  --agent <agent>                       Agent for the current session.
  --model <model>                       Model for the current session. Provide
                                        an alias for the latest model (e.g.
                                        'fable', 'opus', or 'sonnet') or a
                                        model's full name (e.g.
                                        'claude-fable-5').
  -n, --name <name>                     Set a display name for this session
                                        (shown in the prompt box)
"#;

#[test]
fn help_parser_extracts_aliases_and_full_names() {
    let got = crate::brain::provider::claude_cli::parse_models_from_help(REAL_HELP);
    for expected in ["fable", "opus", "sonnet", "claude-fable-5"] {
        assert!(
            got.iter().any(|m| m == expected),
            "missing {expected}: {got:?}"
        );
    }
}

#[test]
fn help_parser_rejects_prose_from_the_shifted_apostrophe() {
    let got = crate::brain::provider::claude_cli::parse_models_from_help(REAL_HELP);
    // "model's" shifts quote parity; prose must never leak in as a model name.
    for m in &got {
        assert!(!m.contains(' '), "prose leaked into the model list: {m:?}");
        assert!(!m.contains("full name"), "prose leaked: {m:?}");
    }
    // And it must not pick up the neighbouring option's text.
    assert!(!got.iter().any(|m| m == "session"), "{got:?}");
}

#[test]
fn help_parser_stops_at_the_next_option_and_handles_missing_block() {
    // No --model block at all → empty, so callers fall back to the const floor.
    assert!(
        crate::brain::provider::claude_cli::parse_models_from_help("Options:\n  -v, --version\n")
            .is_empty()
    );
}

#[test]
fn discovery_never_offers_non_model_sentinels() {
    // A real ~/.opencrabs/claude_cli_models.json was found holding
    // "sonnet": "<synthetic>". Discovery must shape-filter it out rather than
    // offer it as a selectable model (#753).
    let models = crate::brain::provider::claude_cli::available_models();
    for m in &models {
        assert!(
            !m.starts_with('<') && !m.contains(' ') && !m.is_empty(),
            "non-model sentinel leaked into the menu: {m:?}"
        );
    }
}
