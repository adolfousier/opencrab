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
pub mod factory;
pub mod openai;

pub use anthropic::AnthropicProvider;
pub use factory::create_provider;
pub use openai::OpenAIProvider;
