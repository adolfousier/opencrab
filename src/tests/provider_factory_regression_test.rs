//! Provider factory regression tests.
//!
//! These tests verify that all 11 built-in providers are correctly wired
//! across the factory functions. They serve as a regression suite before
//! refactoring the factory into a registry pattern.

use crate::brain::provider::factory::{
    active_provider_vision, create_provider, create_provider_by_name,
};
use crate::config::{Config, ProviderConfig, ProviderConfigs};
use std::collections::BTreeMap;

// ── Helpers ─────────────────────────────────────────────────────

fn config_with_provider(name: &str) -> Config {
    let cfg = ProviderConfig {
        enabled: true,
        api_key: Some("test-key".to_string()),
        base_url: Some("http://localhost:1234/v1".to_string()),
        default_model: Some("test-model".to_string()),
        models: vec![],
        vision_model: None,
        ..Default::default()
    };

    let mut providers = ProviderConfigs::default();
    match name {
        "claude_cli" => providers.claude_cli = Some(cfg),
        "opencode_cli" => providers.opencode_cli = Some(cfg),
        "codex_cli" => providers.codex_cli = Some(cfg),
        "qwen" => providers.qwen = Some(cfg),
        "anthropic" => providers.anthropic = Some(cfg),
        "openai" => providers.openai = Some(cfg),
        "github" => providers.github = Some(cfg),
        "gemini" => providers.gemini = Some(cfg),
        "openrouter" => providers.openrouter = Some(cfg),
        "minimax" => providers.minimax = Some(cfg),
        "zhipu" => providers.zhipu = Some(cfg),
        "ollama" => providers.ollama = Some(cfg),
        _ => {}
    }
    Config {
        providers,
        ..Default::default()
    }
}

// ── create_provider_by_name: session ID resolution ──────────────

#[tokio::test]
async fn by_name_anthropic() {
    let config = config_with_provider("anthropic");
    let result = create_provider_by_name(&config, "anthropic").await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().name(), "anthropic");
}

#[tokio::test]
async fn by_name_openai() {
    let config = config_with_provider("openai");
    let result = create_provider_by_name(&config, "openai").await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().name(), "openai");
}

#[tokio::test]
async fn by_name_github() {
    let config = config_with_provider("github");
    let result = create_provider_by_name(&config, "github").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn by_name_gemini() {
    let config = config_with_provider("gemini");
    let result = create_provider_by_name(&config, "gemini").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn by_name_openrouter() {
    let config = config_with_provider("openrouter");
    let result = create_provider_by_name(&config, "openrouter").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn by_name_minimax() {
    let config = config_with_provider("minimax");
    let result = create_provider_by_name(&config, "minimax").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn by_name_zhipu() {
    let config = config_with_provider("zhipu");
    let result = create_provider_by_name(&config, "zhipu").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn by_name_qwen() {
    let config = config_with_provider("qwen");
    let result = create_provider_by_name(&config, "qwen").await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().name(), "qwen");
}

#[tokio::test]
async fn by_name_ollama() {
    let config = config_with_provider("ollama");
    let result = create_provider_by_name(&config, "ollama").await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().name(), "ollama");
}

// ── create_provider_by_name: alias resolution ───────────────────

#[tokio::test]
async fn by_name_claude_cli_hyphen() {
    // Claude CLI name resolution should work regardless of binary presence.
    // If binary exists → Ok with name "claude-cli". If not → Err about binary.
    // Must NOT return "unknown provider".
    let config = Config::default();
    let result = create_provider_by_name(&config, "claude-cli").await;
    match &result {
        Ok(p) => assert_eq!(p.name(), "claude-cli"),
        Err(e) => {
            let err = e.to_string();
            assert!(
                err.contains("binary") || err.contains("configured") || err.contains("not found"),
                "Expected claude-cli resolution error, got: {}",
                err
            );
        }
    }
}

#[tokio::test]
async fn by_name_claude_cli_underscore() {
    let config = Config::default();
    let result = create_provider_by_name(&config, "claude_cli").await;
    match &result {
        Ok(p) => assert_eq!(p.name(), "claude-cli"),
        Err(e) => {
            let err = e.to_string();
            assert!(
                err.contains("binary") || err.contains("configured") || err.contains("not found"),
                "Expected claude_cli resolution error, got: {}",
                err
            );
        }
    }
}

#[tokio::test]
async fn by_name_opencode_cli_hyphen() {
    let config = Config::default();
    let result = create_provider_by_name(&config, "opencode-cli").await;
    match &result {
        Ok(p) => assert_eq!(p.name(), "opencode"),
        Err(e) => {
            let err = e.to_string();
            assert!(
                err.contains("binary") || err.contains("configured") || err.contains("not found"),
                "Expected opencode-cli resolution error, got: {}",
                err
            );
        }
    }
}

#[tokio::test]
async fn by_name_opencode_cli_underscore() {
    let config = Config::default();
    let result = create_provider_by_name(&config, "opencode_cli").await;
    match &result {
        Ok(p) => assert_eq!(p.name(), "opencode"),
        Err(e) => {
            let err = e.to_string();
            assert!(
                err.contains("binary") || err.contains("configured") || err.contains("not found"),
                "Expected opencode_cli resolution error, got: {}",
                err
            );
        }
    }
}

#[tokio::test]
async fn by_name_codex_cli_hyphen() {
    let config = Config::default();
    let result = create_provider_by_name(&config, "codex-cli").await;
    match &result {
        Ok(p) => assert_eq!(p.name(), "codex-cli"),
        Err(e) => {
            let err = e.to_string();
            assert!(
                err.contains("binary") || err.contains("configured") || err.contains("not found"),
                "Expected codex-cli resolution error, got: {}",
                err
            );
        }
    }
}

#[tokio::test]
async fn by_name_codex_cli_underscore() {
    let config = Config::default();
    let result = create_provider_by_name(&config, "codex_cli").await;
    match &result {
        Ok(p) => assert_eq!(p.name(), "codex-cli"),
        Err(e) => {
            let err = e.to_string();
            assert!(
                err.contains("binary") || err.contains("configured") || err.contains("not found"),
                "Expected codex_cli resolution error, got: {}",
                err
            );
        }
    }
}

#[tokio::test]
async fn by_name_custom_prefix() {
    let mut custom_map = BTreeMap::new();
    custom_map.insert(
        "mylocal".to_string(),
        ProviderConfig {
            enabled: true,
            base_url: Some("http://localhost:1234/v1".to_string()),
            ..Default::default()
        },
    );
    let config = Config {
        providers: ProviderConfigs {
            custom: Some(custom_map),
            ..Default::default()
        },
        ..Default::default()
    };
    let result = create_provider_by_name(&config, "custom:mylocal").await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().name(), "mylocal");
}

// ── create_provider: priority order ─────────────────────────────

#[tokio::test]
async fn priority_anthropic_over_openai() {
    let config = Config {
        providers: ProviderConfigs {
            anthropic: Some(ProviderConfig {
                enabled: true,
                api_key: Some("anthropic-key".to_string()),
                ..Default::default()
            }),
            openai: Some(ProviderConfig {
                enabled: true,
                api_key: Some("openai-key".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let result = create_provider(&config).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().name(), "anthropic");
}

#[tokio::test]
async fn priority_openai_over_gemini() {
    let config = Config {
        providers: ProviderConfigs {
            openai: Some(ProviderConfig {
                enabled: true,
                api_key: Some("openai-key".to_string()),
                ..Default::default()
            }),
            gemini: Some(ProviderConfig {
                enabled: true,
                api_key: Some("gemini-key".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let result = create_provider(&config).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().name(), "openai");
}

#[tokio::test]
async fn priority_gemini_over_openrouter() {
    let config = Config {
        providers: ProviderConfigs {
            gemini: Some(ProviderConfig {
                enabled: true,
                api_key: Some("gemini-key".to_string()),
                ..Default::default()
            }),
            openrouter: Some(ProviderConfig {
                enabled: true,
                api_key: Some("or-key".to_string()),
                base_url: Some("https://openrouter.ai/api/v1/chat/completions".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let result = create_provider(&config).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().name(), "gemini");
}

#[tokio::test]
async fn priority_minimax_over_zhipu() {
    let config = Config {
        providers: ProviderConfigs {
            minimax: Some(ProviderConfig {
                enabled: true,
                api_key: Some("minimax-key".to_string()),
                base_url: Some("https://api.minimax.io/v1".to_string()),
                ..Default::default()
            }),
            zhipu: Some(ProviderConfig {
                enabled: true,
                api_key: Some("zhipu-key".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let result = create_provider(&config).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().name(), "minimax");
}

#[tokio::test]
async fn disabled_provider_skipped() {
    // Anthropic disabled, OpenAI enabled — should pick OpenAI
    let config = Config {
        providers: ProviderConfigs {
            anthropic: Some(ProviderConfig {
                enabled: false,
                api_key: Some("anthropic-key".to_string()),
                ..Default::default()
            }),
            openai: Some(ProviderConfig {
                enabled: true,
                api_key: Some("openai-key".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let result = create_provider(&config).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().name(), "openai");
}

#[tokio::test]
async fn no_provider_returns_placeholder() {
    let config = Config::default();
    let result = create_provider(&config).await;
    assert!(result.is_ok());
    // PlaceholderProvider::name() returns "none"
    assert_eq!(result.unwrap().name(), "none");
}

// ── TUI / Factory consistency ──────────────────────────────────

#[test]
fn tui_providers_match_factory_session_ids() {
    // Every static PROVIDER id in the TUI must have a matching session_id
    // in the factory's REGISTRATIONS. The last entry (Custom, id="") is excluded.
    // This catches cases where a provider is added to one layer but not the other.
    use crate::tui::onboarding::PROVIDERS;
    let session_ids = crate::brain::provider::factory::provider_session_ids();

    for p in PROVIDERS.iter() {
        if p.id.is_empty() {
            continue; // Custom sentinel — dynamic name, skip
        }
        assert!(
            session_ids.contains(&p.id),
            "TUI provider id '{}' not found in factory session_ids",
            p.id
        );
    }
}

// ── active_provider_vision ──────────────────────────────────────

#[test]
fn vision_anthropic() {
    let config = Config {
        providers: ProviderConfigs {
            anthropic: Some(ProviderConfig {
                enabled: true,
                api_key: Some("key".to_string()),
                vision_model: Some("claude-3-opus".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let result = active_provider_vision(&config);
    assert!(result.is_some());
    let (key, url, model) = result.unwrap();
    assert_eq!(key, "key");
    assert_eq!(model, "claude-3-opus");
    assert!(url.contains("chat/completions"));
}

#[test]
fn vision_openai() {
    let config = Config {
        providers: ProviderConfigs {
            openai: Some(ProviderConfig {
                enabled: true,
                api_key: Some("key".to_string()),
                vision_model: Some("gpt-4o".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let result = active_provider_vision(&config);
    assert!(result.is_some());
    let (_, _, model) = result.unwrap();
    assert_eq!(model, "gpt-4o");
}

#[test]
fn vision_openrouter() {
    let config = Config {
        providers: ProviderConfigs {
            openrouter: Some(ProviderConfig {
                enabled: true,
                api_key: Some("key".to_string()),
                base_url: Some("https://openrouter.ai/api/v1/chat/completions".to_string()),
                vision_model: Some("anthropic/claude-3.5-sonnet".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let result = active_provider_vision(&config);
    assert!(result.is_some());
    let (_, _, model) = result.unwrap();
    assert_eq!(model, "anthropic/claude-3.5-sonnet");
}

#[test]
fn vision_minimax() {
    let config = Config {
        providers: ProviderConfigs {
            minimax: Some(ProviderConfig {
                enabled: true,
                api_key: Some("key".to_string()),
                base_url: Some("https://api.minimax.io/v1".to_string()),
                vision_model: Some("MiniMax-Text-01".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let result = active_provider_vision(&config);
    assert!(result.is_some());
    let (_, _, model) = result.unwrap();
    assert_eq!(model, "MiniMax-Text-01");
}

#[test]
fn vision_none_when_no_vision_model() {
    let config = Config {
        providers: ProviderConfigs {
            anthropic: Some(ProviderConfig {
                enabled: true,
                api_key: Some("key".to_string()),
                vision_model: None,
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let result = active_provider_vision(&config);
    assert!(result.is_none());
}

#[test]
fn vision_none_when_no_api_key() {
    let config = Config {
        providers: ProviderConfigs {
            openai: Some(ProviderConfig {
                enabled: true,
                api_key: None,
                vision_model: Some("gpt-4o".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let result = active_provider_vision(&config);
    assert!(result.is_none());
}

#[test]
fn vision_custom_provider() {
    let mut custom_map = BTreeMap::new();
    custom_map.insert(
        "myprovider".to_string(),
        ProviderConfig {
            enabled: true,
            api_key: Some("custom-key".to_string()),
            base_url: Some("http://localhost:8080/v1".to_string()),
            vision_model: Some("custom-vision-model".to_string()),
            ..Default::default()
        },
    );
    let config = Config {
        providers: ProviderConfigs {
            custom: Some(custom_map),
            ..Default::default()
        },
        ..Default::default()
    };
    // active_provider_vision uses active_provider_and_model which picks
    // the first enabled custom provider. The session_id for custom is
    // "custom:<name>".
    // Note: active_provider_vision checks the active provider from config,
    // not a specific name. Custom providers are picked via active_custom().
    // This test verifies the custom: prefix routing works.
    let result = active_provider_vision(&config);
    assert!(result.is_some());
    let (key, _, model) = result.unwrap();
    assert_eq!(key, "custom-key");
    assert_eq!(model, "custom-vision-model");
}
