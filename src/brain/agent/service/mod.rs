//! Agent Service Implementation
//!
//! Core service for managing AI agent conversations, coordinating between
//! LLM providers, context management, and data persistence.

mod builder;
mod compaction;
mod context;
mod feedback;
mod gaslighting;
pub(crate) mod helpers;
mod messaging;
mod phantom;
pub(crate) mod tool_loop;
mod types;

pub use builder::AgentService;
pub use gaslighting::{is_gaslighting_preamble, strip_gaslighting_preamble};
pub use helpers::detect_text_repetition;
pub use phantom::{
    has_investigative_intent, has_phantom_tool_intent, has_phantom_tool_intent_no_tools,
    looks_truncated_mid_sentence,
};
pub use types::{
    AgentResponse, AgentStreamResponse, ApprovalCallback, ChannelSessionEvent,
    MessageQueueCallback, ProgressCallback, ProgressEvent, SshPasswordCallback, SudoCallback,
    ToolApprovalInfo,
};
