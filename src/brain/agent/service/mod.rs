//! Agent Service Implementation
//!
//! Core service for managing AI agent conversations, coordinating between
//! LLM providers, context management, and data persistence.

mod builder;
mod compaction;
pub(crate) mod compaction_prompts;
pub(crate) mod context;
#[allow(unused_imports)] // only used in test code
pub(crate) use context::format_plan_reminder;
pub(crate) mod feedback;
mod gaslighting;
pub(crate) mod helpers;
mod messaging;
pub(crate) mod phantom;
pub(crate) mod phantom_lang;
pub(crate) mod tool_loop;
mod truncation;
mod types;

pub use builder::{AgentService, BrainRebuild};
pub use gaslighting::{is_gaslighting_preamble, strip_gaslighting_preamble};
pub use helpers::detect_text_repetition;
pub use phantom::{
    count_intent_line_starts, has_forward_intent_post_success, has_investigative_intent,
    has_phantom_tool_intent, has_phantom_tool_intent_no_tools, is_analysis_intent,
    is_stuck_in_intent_loop, looks_truncated_mid_sentence,
};
pub use types::{
    AgentResponse, AgentStreamResponse, ApprovalCallback, ChannelSessionEvent,
    FollowUpQuestionInfo, MessageQueueCallback, ProgressCallback, ProgressEvent, QuestionCallback,
    SshPasswordCallback, SudoCallback, ToolApprovalInfo,
};
