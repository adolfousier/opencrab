//! Context-manifest trace hook (#620).
//!
//! [`ContextManifest`] captures WHAT went into a built [`LLMRequest`]: the
//! system-brain size, which brain files are present, the message-token split,
//! and the tool roster with per-tool schema cost and CORE-vs-extended counts.
//!
//! It is derived purely from the request that is about to be sent
//! ([`ContextManifest::from_request`]), so an eval can inspect context
//! composition without altering any production path — the harness simply builds
//! the same request the agent would and analyzes it.

use serde::Serialize;

use crate::brain::provider::{ContentBlock, LLMRequest, Message};
use crate::brain::tokenizer::count_tokens;
use crate::brain::tools::catalog::is_core;

/// Brain files whose presence in the system prompt we detect by name.
const BRAIN_FILE_NAMES: &[&str] = &[
    "SOUL.md",
    "USER.md",
    "AGENTS.md",
    "TOOLS.md",
    "CODE.md",
    "SECURITY.md",
    "MEMORY.md",
    "BOOT.md",
    "HEARTBEAT.md",
];

/// Flat per-image token estimate, matching the agent context accounting.
const IMAGE_TOKEN_ESTIMATE: usize = 1000;

/// Per-message framing overhead, matching `tokenizer::count_message_tokens`.
const MESSAGE_OVERHEAD_TOKENS: usize = 4;

/// One tool as it appears in the request, with its schema token cost.
#[derive(Debug, Clone, Serialize)]
pub struct ToolEntry {
    pub name: String,
    pub schema_tokens: usize,
    pub is_core: bool,
}

/// A structured view of a built request's context composition.
#[derive(Debug, Clone, Serialize)]
pub struct ContextManifest {
    pub system_brain_bytes: usize,
    pub system_brain_tokens: usize,
    /// Brain files detected in the system prompt (by filename), in canonical
    /// order.
    pub brain_files_present: Vec<String>,
    pub message_count: usize,
    pub message_tokens: usize,
    pub tools: Vec<ToolEntry>,
    pub tool_schema_tokens: usize,
    pub core_tool_count: usize,
    pub extended_tool_count: usize,
    /// system + messages + tool schemas (cl100k estimate).
    pub total_input_tokens: usize,
}

impl ContextManifest {
    /// Analyze the request that is about to be sent.
    pub fn from_request(request: &LLMRequest) -> Self {
        let system = request.system.as_deref().unwrap_or("");
        let system_brain_bytes = system.len();
        let system_brain_tokens = count_tokens(system);
        let brain_files_present = BRAIN_FILE_NAMES
            .iter()
            .filter(|n| system.contains(**n))
            .map(|n| n.to_string())
            .collect();

        let message_count = request.messages.len();
        let message_tokens = request.messages.iter().map(message_tokens).sum();

        let mut tools = Vec::new();
        let mut tool_schema_tokens = 0;
        let mut core_tool_count = 0;
        let mut extended_tool_count = 0;
        if let Some(defs) = &request.tools {
            for t in defs {
                let schema = serde_json::to_string(&t.input_schema).unwrap_or_default();
                let schema_tokens =
                    count_tokens(&t.name) + count_tokens(&t.description) + count_tokens(&schema);
                let core = is_core(&t.name);
                if core {
                    core_tool_count += 1;
                } else {
                    extended_tool_count += 1;
                }
                tool_schema_tokens += schema_tokens;
                tools.push(ToolEntry {
                    name: t.name.clone(),
                    schema_tokens,
                    is_core: core,
                });
            }
        }

        let total_input_tokens = system_brain_tokens + message_tokens + tool_schema_tokens;
        Self {
            system_brain_bytes,
            system_brain_tokens,
            brain_files_present,
            message_count,
            message_tokens,
            tools,
            tool_schema_tokens,
            core_tool_count,
            extended_tool_count,
            total_input_tokens,
        }
    }

    /// Whether a tool with this name is present in the request.
    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.iter().any(|t| t.name == name)
    }

    /// Whether a brain file with this name was detected in the system prompt.
    pub fn has_brain_file(&self, name: &str) -> bool {
        self.brain_files_present.iter().any(|f| f == name)
    }
}

/// Token estimate for one message, mirroring the agent context accounting:
/// per-block content tokens plus a fixed framing overhead.
fn message_tokens(m: &Message) -> usize {
    let mut total = MESSAGE_OVERHEAD_TOKENS;
    for block in &m.content {
        total += match block {
            ContentBlock::Text { text } => count_tokens(text),
            ContentBlock::ToolUse { name, input, .. } => {
                count_tokens(name) + count_tokens(&input.to_string())
            }
            ContentBlock::ToolResult { content, .. } => count_tokens(content),
            ContentBlock::Thinking { thinking, .. } => count_tokens(thinking),
            ContentBlock::Image { .. } => IMAGE_TOKEN_ESTIMATE,
        };
    }
    total
}
