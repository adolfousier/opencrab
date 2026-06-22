//! Agent Service Module
//!
//! Provides high-level agent functionality for managing conversations,
//! executing tools, and coordinating with LLM providers.

pub mod context;
pub mod error;
pub mod service;

// Re-exports
pub use context::AgentContext;
pub use error::{AgentError, Result, format_user_error};
pub use service::{
    AgentResponse, AgentService, AgentStreamResponse, ApprovalCallback, BrainRebuild,
    ChannelSessionEvent, FollowUpQuestionInfo, MessageQueueCallback, ProgressCallback,
    ProgressEvent, QuestionCallback, SshPasswordCallback, SudoCallback, ToolApprovalInfo,
};
