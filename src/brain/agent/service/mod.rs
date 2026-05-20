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
mod phantom_lang;
pub(crate) mod tool_loop;
mod truncation;
mod types;

pub use builder::AgentService;
pub use gaslighting::{is_gaslighting_preamble, strip_gaslighting_preamble};
pub use helpers::detect_text_repetition;
pub use phantom::{
    count_intent_line_starts, has_investigative_intent, has_phantom_tool_intent,
    has_phantom_tool_intent_no_tools, is_stuck_in_intent_loop, looks_truncated_mid_sentence,
};
pub use types::{
    AgentResponse, AgentStreamResponse, ApprovalCallback, ChannelSessionEvent,
    FollowUpQuestionInfo, MessageQueueCallback, ProgressCallback, ProgressEvent, QuestionCallback,
    SshPasswordCallback, SudoCallback, ToolApprovalInfo,
};
