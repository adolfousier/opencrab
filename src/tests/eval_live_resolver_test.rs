//! Tests for live-eval provider resolution (live-L0, #629).

use crate::config::{Config, ProviderConfig, ProviderConfigs};
use crate::eval::live::resolve_eval_providers;

fn provider(model: &str) -> ProviderConfig {
    ProviderConfig {
        enabled: true,
        api_key: Some("test-key".to_string()),
        default_model: Some(model.to_string()),
        ..Default::default()
    }
}

#[tokio::test]
async fn empty_chain_resolves_to_nothing() {
    let config = Config::default();
    assert!(resolve_eval_providers(&config).await.is_empty());
}

#[tokio::test]
async fn resolves_configured_providers_with_their_default_models() {
    let mut config = Config {
        providers: ProviderConfigs {
            anthropic: Some(provider("claude-opus-4-8")),
            minimax: Some(provider("MiniMax-M2.7")),
            ..Default::default()
        },
        ..Default::default()
    };
    config.agent.eval_providers = vec!["anthropic".to_string(), "minimax".to_string()];

    let providers = resolve_eval_providers(&config).await;
    assert_eq!(providers.len(), 2);
    // Each judge uses its own configured default_model ("set at their levels").
    assert_eq!(providers[0].default_model(), "claude-opus-4-8");
    assert_eq!(providers[1].default_model(), "MiniMax-M2.7");
}

#[tokio::test]
async fn unconfigured_name_is_skipped_not_fatal() {
    let mut config = Config {
        providers: ProviderConfigs {
            anthropic: Some(provider("claude-opus-4-8")),
            ..Default::default()
        },
        ..Default::default()
    };
    // "zhipu" is not configured; it must be skipped, anthropic still resolves.
    config.agent.eval_providers = vec!["zhipu".to_string(), "anthropic".to_string()];

    let providers = resolve_eval_providers(&config).await;
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].default_model(), "claude-opus-4-8");
}

#[tokio::test]
async fn duplicates_and_blanks_are_removed_in_order() {
    let mut config = Config {
        providers: ProviderConfigs {
            anthropic: Some(provider("claude-opus-4-8")),
            ..Default::default()
        },
        ..Default::default()
    };
    config.agent.eval_providers = vec![
        "  ".to_string(),
        "anthropic".to_string(),
        "anthropic".to_string(),
    ];

    let providers = resolve_eval_providers(&config).await;
    assert_eq!(providers.len(), 1);
}
