//! Brain Module
//!
//! The core intelligence layer — LLM providers, agent services, tools, tokenizer,
//! dynamic system prompt assembly, user-defined slash commands, and self-update.

pub mod agent;
pub mod commands;
pub mod mission_control;
pub mod prompt_builder;
pub mod provider;
pub mod rsi;
pub mod rsi_git_history;
pub mod rsi_proposals;
pub mod rsi_sync;
pub mod self_update;
pub mod skills;
pub mod tokenizer;
pub mod tools;

// Brain re-exports
pub use commands::{CommandLoader, UserCommand};
pub use prompt_builder::BrainLoader;
pub use self_update::SelfUpdater;

// LLM re-exports
pub use agent::{AgentContext, AgentError, AgentService};
pub use provider::{
    AnthropicProvider, ContentBlock, LLMRequest, LLMResponse, Message, Provider, ProviderError,
    ProviderStream, Role, StopReason, StreamEvent, TokenUsage, Tool,
};
pub use tools::{ToolError, ToolRegistry, ToolResult};
