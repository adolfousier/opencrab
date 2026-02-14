//! LLM Integration Layer
//!
//! Provides abstraction over multiple LLM providers (Anthropic, OpenAI, etc.)
//! and agent services for handling conversations, tool execution, and context management.

pub mod agent;
pub mod provider;
pub mod tools;

// Re-exports for convenience
pub use provider::{
    AnthropicProvider, ContentBlock, LLMRequest, LLMResponse, Message, Provider, ProviderError,
    ProviderStream, Role, StopReason, StreamEvent, TokenUsage, Tool,
};

pub use agent::{AgentContext, AgentError, AgentService};
pub use tools::{ToolError, ToolRegistry, ToolResult};
