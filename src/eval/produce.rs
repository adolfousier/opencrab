//! Live artifact producers (live-L2, #631).
//!
//! Produces a REAL artifact from a live provider so an eval grades real model
//! output rather than a fixture. [`produce_compaction_summary`] asks a provider
//! to compact a conversation into a continuation document, then the compaction
//! dataset's probes grade what survived.
//!
//! The continuation-document instruction mirrors the SECTION STRUCTURE of the
//! production compaction prompt (agent/service/context.rs) — immediate task,
//! files modified, user preferences, errors, pending tasks — without its live
//! token-budget preamble, which is meaningless for a fixed dataset. Kept
//! self-contained here so the production compaction path is untouched; sharing
//! the exact prompt is a possible later refinement.

use crate::brain::provider::{ContentBlock, LLMRequest, Message, Provider, Tool};

use super::compaction::CompactionDataset;
use super::scorer::{Judge, Scorecard};

/// Instruction appended to a conversation asking the model to compact it into a
/// fact-preserving continuation document.
pub const CONTINUATION_INSTRUCTION: &str = "The conversation must be compacted now. Produce a \
     COMPREHENSIVE CONTINUATION DOCUMENT so a fresh agent can resume with only this summary. \
     Analyze the entire conversation and preserve, with exact detail:\n\
     ## Immediate task — the user's last instruction and the exact next action.\n\
     ## Files modified — every file created/edited/read, with paths and what changed.\n\
     ## User preferences & constraints — everything the user said to do or never do.\n\
     ## Errors & corrections — every error message and how it was resolved.\n\
     ## Pending tasks — everything not yet done.\n\
     Preserve exact identifiers (file paths, error codes, commands) verbatim.";

/// A representative tool set for behavioral evals — the config tooling a
/// self-aware agent should reach for (config_manager, tool_search,
/// load_brain_file) plus the generic tools a reimplementing agent would misuse
/// (bash, write_file). Descriptions mirror the real tools so the model behaves
/// as it would at runtime. Not the full registry — just enough to discriminate
/// "configure the built-in" from "build a replacement".
pub fn eval_tool_set() -> Vec<Tool> {
    let tool = |name: &str, description: &str, schema: serde_json::Value| Tool {
        name: name.to_string(),
        description: description.to_string(),
        input_schema: schema,
    };
    vec![
        tool(
            "config_manager",
            "Read or write OpenCrabs configuration (config.toml): enable and configure providers, \
             compiled features, channels, STT/TTS, and models.",
            serde_json::json!({"type":"object","properties":{"operation":{"type":"string"},"section":{"type":"string"},"key":{"type":"string"},"value":{"type":"string"}}}),
        ),
        tool(
            "tool_search",
            "Discover available tools by task description; returns matching tool schemas to activate.",
            serde_json::json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}),
        ),
        tool(
            "load_brain_file",
            "Load an OpenCrabs brain file (TOOLS.md, CODE.md, SECURITY.md, ...) for guidance.",
            serde_json::json!({"type":"object","properties":{"name":{"type":"string"}}}),
        ),
        tool(
            "bash",
            "Run a shell command.",
            serde_json::json!({"type":"object","properties":{"command":{"type":"string"}},"required":["command"]}),
        ),
        tool(
            "write_file",
            "Create a new file with the given content.",
            serde_json::json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}}}),
        ),
    ]
}

/// Extract the concatenated text of a provider response.
fn response_text(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Render a response for grading, including any TOOL CALLS the model made. A
/// tool-calling agent takes action by calling tools, not narrating — so the
/// grader must see the calls (e.g. `[tool_call] config_manager {…}`), or a
/// correct "call config_manager to enable local-stt" looks like an empty stub.
fn render_response(blocks: &[ContentBlock]) -> String {
    let mut out = String::new();
    for b in blocks {
        match b {
            ContentBlock::Text { text } if !text.trim().is_empty() => {
                out.push_str(text);
                out.push('\n');
            }
            ContentBlock::ToolUse { name, input, .. } => {
                out.push_str(&format!("[tool_call] {name} {input}\n"));
            }
            _ => {}
        }
    }
    out.trim().to_string()
}

/// Send a single user prompt to a live provider and return its rendered
/// response (text + any tool calls). When `tools` is non-empty the model may
/// call them — the whole point for a tool-calling agent — and the calls are
/// folded into the returned string so the grader sees the action taken. When
/// `system` is set (e.g. the real OpenCrabs system brain) it is attached. A
/// provider error returns a marked failure string, never a silent empty.
pub async fn produce_response(
    provider: &dyn Provider,
    model: &str,
    prompt: &str,
    system: Option<&str>,
    tools: &[Tool],
) -> String {
    let mut request = LLMRequest::new(model.to_string(), vec![Message::user(prompt.to_string())]);
    if let Some(sys) = system {
        request = request.with_system(sys);
    }
    if !tools.is_empty() {
        request = request.with_tools(tools.to_vec());
    }
    match provider.complete(request).await {
        Ok(resp) => render_response(&resp.content),
        Err(e) => format!("[produce failed: {e}]"),
    }
}

/// Ask a live provider to compact a conversation into a continuation document.
/// When `system` is set it is attached (e.g. the real system brain). A provider
/// error returns a marked failure string (never a silent empty), so the grader
/// scores it as full fact loss rather than a spurious pass.
pub async fn produce_compaction_summary(
    provider: &dyn Provider,
    model: &str,
    conversation: &[Message],
    system: Option<&str>,
) -> String {
    let mut messages = conversation.to_vec();
    messages.push(Message::user(CONTINUATION_INSTRUCTION.to_string()));
    let mut request = LLMRequest::new(model.to_string(), messages);
    if let Some(sys) = system {
        request = request.with_system(sys);
    }
    match provider.complete(request).await {
        Ok(resp) => response_text(&resp.content),
        Err(e) => format!("[compaction failed: {e}]"),
    }
}

/// End-to-end: produce a real compaction summary with `producer`, then grade the
/// dataset's probes against it with `judge`.
pub async fn run_compaction_eval(
    producer: &dyn Provider,
    producer_model: &str,
    judge: &dyn Judge,
    dataset: &CompactionDataset,
    system: Option<&str>,
) -> Scorecard {
    let summary =
        produce_compaction_summary(producer, producer_model, &dataset.messages(), system).await;
    dataset.judge_scorecard(judge, &summary).await
}
