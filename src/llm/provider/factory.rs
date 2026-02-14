//! Provider Factory
//!
//! Centralized provider creation logic to reduce code duplication.

use super::{
    anthropic::AnthropicProvider,
    openai::OpenAIProvider,
    qwen::{QwenProvider, ToolCallParser},
    Provider,
};
use crate::config::{Config, ProviderConfig, QwenProviderConfig};
use anyhow::{Context, Result};
use std::sync::Arc;

/// Create a provider based on configuration with fallback priority
///
/// Priority order:
/// 1. Qwen (if configured with credentials)
/// 2. OpenAI (if configured with credentials)
/// 3. Anthropic (default fallback)
pub fn create_provider(config: &Config) -> Result<Arc<dyn Provider>> {
    // Try Qwen first
    if let Some(provider) = try_create_qwen(config)? {
        return Ok(provider);
    }

    // Try OpenAI
    if let Some(provider) = try_create_openai(config)? {
        return Ok(provider);
    }

    // Fall back to Anthropic
    create_anthropic(config)
}

/// Try to create Qwen provider if configured
fn try_create_qwen(config: &Config) -> Result<Option<Arc<dyn Provider>>> {
    let qwen_config = match &config.providers.qwen {
        Some(cfg) => cfg,
        None => return Ok(None),
    };

    // Local Qwen (vLLM, LM Studio, etc.)
    if let Some(base_url) = &qwen_config.base_url {
        tracing::info!("Using local Qwen at: {}", base_url);
        println!("🏠 Using local Qwen at: {}\n", base_url);

        let provider = configure_qwen(QwenProvider::local(base_url.clone()), qwen_config);
        return Ok(Some(Arc::new(provider)));
    }

    // DashScope cloud API
    if let Some(api_key) = &qwen_config.api_key {
        let region = qwen_config.region.as_deref().unwrap_or("intl");

        let provider_base = match region {
            "cn" => {
                tracing::info!("Using DashScope China (Beijing)");
                println!("☁️  Using DashScope China (Beijing)\n");
                QwenProvider::dashscope_cn(api_key.clone())
            }
            _ => {
                tracing::info!("Using DashScope International (Singapore)");
                println!("☁️  Using DashScope International (Singapore)\n");
                QwenProvider::dashscope_intl(api_key.clone())
            }
        };

        let provider = configure_qwen(provider_base, qwen_config);
        return Ok(Some(Arc::new(provider)));
    }

    Ok(None)
}

/// Configure Qwen provider with tool parser, thinking mode, and model
fn configure_qwen(mut provider: QwenProvider, config: &QwenProviderConfig) -> QwenProvider {
    // Set tool parser
    if let Some(parser) = &config.tool_parser {
        let tool_parser = match parser.as_str() {
            "openai" => ToolCallParser::OpenAI,
            "native" | "qwen" => ToolCallParser::NativeQwen,
            _ => ToolCallParser::Hermes,
        };
        provider = provider.with_tool_parser(tool_parser);
        tracing::info!("Using tool parser: {:?}", tool_parser);

        if tool_parser == ToolCallParser::NativeQwen {
            println!("🔧 Using native Qwen function calling (✿FUNCTION✿ markers)\n");
        }
    }

    // Set thinking mode
    if config.enable_thinking {
        provider = provider.with_thinking(true);
        tracing::info!("🧠 Qwen3 thinking mode enabled");
        println!("🧠 Thinking mode: enabled\n");

        if let Some(budget) = config.thinking_budget {
            provider = provider.with_thinking_budget(budget);
            tracing::info!("Thinking budget: {} tokens", budget);
        }
    }

    // Set custom model
    if let Some(model) = &config.default_model {
        tracing::info!("Using custom default model: {}", model);
        println!("📦 Model: {}\n", model);
        provider = provider.with_default_model(model.clone());
    }

    provider
}

/// Try to create OpenAI provider if configured
fn try_create_openai(config: &Config) -> Result<Option<Arc<dyn Provider>>> {
    let openai_config = match &config.providers.openai {
        Some(cfg) => cfg,
        None => return Ok(None),
    };

    // Local LLM (LM Studio, Ollama, etc.)
    if let Some(base_url) = &openai_config.base_url {
        tracing::info!("Using local LLM at: {}", base_url);
        println!("🏠 Using local LLM at: {}\n", base_url);

        let provider = configure_openai(OpenAIProvider::local(base_url.clone()), openai_config);
        return Ok(Some(Arc::new(provider)));
    }

    // Official OpenAI API
    if let Some(api_key) = &openai_config.api_key {
        tracing::info!("Using OpenAI provider");
        println!("🤖 Using OpenAI provider\n");

        let provider = configure_openai(OpenAIProvider::new(api_key.clone()), openai_config);
        return Ok(Some(Arc::new(provider)));
    }

    Ok(None)
}

/// Configure OpenAI provider with custom model
fn configure_openai(mut provider: OpenAIProvider, config: &ProviderConfig) -> OpenAIProvider {
    if let Some(model) = &config.default_model {
        tracing::info!("Using custom default model: {}", model);
        println!("📦 Model: {}\n", model);
        provider = provider.with_default_model(model.clone());
    }
    provider
}

/// Create Anthropic provider (default fallback)
fn create_anthropic(config: &Config) -> Result<Arc<dyn Provider>> {
    let anthropic_config = config.providers.anthropic.as_ref().context(
        "No provider configured.\n\nPlease set one of:\n  - ANTHROPIC_API_KEY for Claude\n  - ANTHROPIC_MAX_SETUP_TOKEN for Claude Max (OAuth)\n  - OPENAI_API_KEY for OpenAI/GPT\n  - OPENAI_BASE_URL for local LLMs (LM Studio, Ollama)\n  - QWEN_BASE_URL for local Qwen (vLLM)\n  - DASHSCOPE_API_KEY for DashScope cloud\n\nExample for vLLM with Qwen:\n  export QWEN_BASE_URL=\"http://localhost:8000/v1/chat/completions\"",
    )?;

    let api_key = anthropic_config
        .api_key
        .as_ref()
        .context("Anthropic API key not set")?
        .clone();

    let mut provider = AnthropicProvider::new(api_key);

    if let Some(model) = &anthropic_config.default_model {
        tracing::info!("Using custom default model: {}", model);
        println!("📦 Model: {}\n", model);
        provider = provider.with_default_model(model.clone());
    }

    tracing::info!("Using Anthropic provider");
    println!("🤖 Using Anthropic Claude\n");

    Ok(Arc::new(provider))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, ProviderConfig, ProviderConfigs, QwenProviderConfig};

    #[test]
    fn test_create_provider_with_anthropic() {
        let config = Config {
            providers: ProviderConfigs {
                anthropic: Some(ProviderConfig {
                    enabled: true,
                    api_key: Some("test-key".to_string()),
                    base_url: None,
                    default_model: None,
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        let result = create_provider(&config);
        assert!(result.is_ok());
        let provider = result.unwrap();
        assert_eq!(provider.name(), "anthropic");
    }

    #[test]
    fn test_create_provider_with_openai() {
        let config = Config {
            providers: ProviderConfigs {
                openai: Some(ProviderConfig {
                    enabled: true,
                    api_key: Some("test-key".to_string()),
                    base_url: None,
                    default_model: None,
                }),
                anthropic: Some(ProviderConfig {
                    enabled: true,
                    api_key: Some("anthropic-key".to_string()),
                    base_url: None,
                    default_model: None,
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        let result = create_provider(&config);
        assert!(result.is_ok());
        let provider = result.unwrap();
        assert_eq!(provider.name(), "openai");
    }

    #[test]
    fn test_create_provider_with_qwen() {
        let config = Config {
            providers: ProviderConfigs {
                qwen: Some(QwenProviderConfig {
                    enabled: true,
                    api_key: Some("test-key".to_string()),
                    base_url: None,
                    default_model: None,
                    tool_parser: None,
                    enable_thinking: false,
                    thinking_budget: None,
                    region: None,
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        let result = create_provider(&config);
        assert!(result.is_ok());
        let provider = result.unwrap();
        assert_eq!(provider.name(), "qwen");
    }

    #[test]
    fn test_create_provider_no_credentials() {
        let config = Config {
            providers: ProviderConfigs {
                anthropic: None,
                openai: None,
                qwen: None,
                ..Default::default()
            },
            ..Default::default()
        };

        let result = create_provider(&config);
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.to_string().contains("No provider configured"));
        }
    }
}
