//! LLM Provider Abstraction Layer
//!
//! Provides a unified interface for interacting with different LLM providers.

pub mod error;
pub mod retry;
#[allow(clippy::module_inception)]
mod r#trait;
pub mod types;

// Re-exports
pub use error::{ProviderError, Result};
pub use r#trait::{Provider, ProviderCapabilities, ProviderStream};
pub use types::*;

// Provider implementations
pub mod anthropic;
pub mod azure;
pub mod factory;
pub mod openai;
pub mod qwen;

pub use anthropic::AnthropicProvider;
pub use azure::AzureOpenAIProvider;
pub use factory::create_provider;
pub use openai::OpenAIProvider;
pub use qwen::{QwenProvider, ThinkingConfig, ToolCallParser};
