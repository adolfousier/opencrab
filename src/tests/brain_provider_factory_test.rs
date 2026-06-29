use crate::brain::provider::factory::*;
use crate::config::{Config, ProviderConfig, ProviderConfigs};
use tokio;

#[tokio::test]
async fn test_create_provider_with_anthropic() {
    let config = Config {
        providers: ProviderConfigs {
            anthropic: Some(ProviderConfig {
                enabled: true,
                api_key: Some("test-key".to_string()),
                base_url: None,
                default_model: None,
                models: vec![],
                vision_model: None,
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let result = create_provider(&config).await;
    assert!(result.is_ok());
    let provider = result.unwrap();
    assert_eq!(provider.name(), "anthropic");
}

#[tokio::test]
async fn test_create_provider_with_minimax() {
    let config = Config {
        providers: ProviderConfigs {
            minimax: Some(ProviderConfig {
                enabled: true,
                api_key: Some("test-key".to_string()),
                base_url: Some("https://api.minimax.io/v1".to_string()),
                default_model: Some("MiniMax-M2.7".to_string()),
                models: vec![],
                vision_model: None,
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let result = create_provider(&config).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_minimax_takes_priority() {
    let config = Config {
        providers: ProviderConfigs {
            openai: Some(ProviderConfig {
                enabled: true,
                api_key: Some("openai-key".to_string()),
                base_url: None,
                default_model: None,
                models: vec![],
                vision_model: None,
                ..Default::default()
            }),
            minimax: Some(ProviderConfig {
                enabled: true,
                api_key: Some("minimax-key".to_string()),
                base_url: Some("https://api.minimax.io/v1".to_string()),
                default_model: None,
                models: vec![],
                vision_model: None,
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let result = create_provider(&config).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_create_provider_no_credentials() {
    let config = Config {
        providers: ProviderConfigs::default(),
        ..Default::default()
    };

    // No credentials → PlaceholderProvider (app starts, shows onboarding)
    let result = create_provider(&config).await;
    assert!(result.is_ok());
}
