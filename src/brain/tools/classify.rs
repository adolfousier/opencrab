//! Shared read-only / destructive classifier for tool calls.
//!
//! Both the approval policy (`requires_approval` in `trait.rs`) and the
//! plan-mode gate (`check_plan_gate` in `plan_gate.rs`) call into this
//! module to answer the same question: "is this tool call destructive?"
//! Keeping the classifier in one place means the two decision layers
//! share a single source of truth for what counts as a side effect,
//! while each layer applies its own policy on top.
//!
//! Future: this is the natural slot for an LLM judge that inspects the
//! actual command text (e.g. `bash ls` = read-only, `bash rm` =
//! destructive) without touching either decision layer.

use super::r#trait::{ToolCapability, ToolHints};

/// The three capability flags that mark a tool as destructive (having
/// side effects beyond reading).
const DESTRUCTIVE_CAPS: [ToolCapability; 3] = [
    ToolCapability::WriteFiles,
    ToolCapability::ExecuteShell,
    ToolCapability::SystemModification,
];

/// Returns `true` if the given capabilities include any destructive
/// (side-effecting) flag: `WriteFiles`, `ExecuteShell`, or
/// `SystemModification`.
pub(crate) fn is_destructive(capabilities: &[ToolCapability]) -> bool {
    capabilities
        .iter()
        .any(|cap| DESTRUCTIVE_CAPS.contains(cap))
}

/// Returns `true` if the given capabilities are purely read-only (no
/// destructive flags). Inverse of [`is_destructive`].
#[allow(dead_code)]
pub(crate) fn is_read_only(capabilities: &[ToolCapability]) -> bool {
    !is_destructive(capabilities)
}

// --- MCP-style behavioral risk classification -------------------------
//
// The Model Context Protocol annotates each tool with orthogonal
// behavioral hints (`readOnlyHint`, `destructiveHint`, `idempotentHint`,
// `openWorldHint`) rather than a flat capability taxonomy. `ToolHints`
// (in `trait.rs`) mirrors that model with pessimistic defaults. These
// hint-based classifiers are the forward path: the capability functions
// above remain only as a migration bridge while tools move from
// `capabilities()` to explicit `hints()`.

/// MCP-style: does this tool have destructive side effects?
///
/// A tool is destructive when it modifies its environment (`read_only ==
/// false`) AND the modification is not purely additive (`destructive ==
/// true`). A read-only tool is never destructive.
pub(crate) fn is_destructive_hints(hints: &ToolHints) -> bool {
    !hints.read_only && hints.destructive
}

/// MCP-style: is this tool purely read-only (no environment modification)?
#[allow(dead_code)]
pub(crate) fn is_read_only_hints(hints: &ToolHints) -> bool {
    hints.read_only
}
