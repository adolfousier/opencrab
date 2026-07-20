//! Plan-mode write/bash gate: the tool-loop enforcement of the Editing
//! write policy and the Active `.md` freeze.
//!
//! Checked on every tool execution (registry choke point). The rules are
//! asymmetric by design:
//!
//! - **Pre-init Editing** (durable flag, no session `.md` yet): deny
//!   project file writes but ALLOW exploratory bash and reads/search so
//!   the agent can investigate before committing to a design doc. `plan`
//!   (for `init`) stays available.
//! - **Post-init Editing** (`.md` + `.json`): allow reads, search,
//!   `follow_up_question`, and writes ONLY to the session `.md`. Deny all
//!   bash, other project writes, system modification, channel sends,
//!   browser mutators, and agent spawn/team mutators.
//! - **Active**: freeze the live design `.md` against generic write
//!   tools; everything else follows the normal approval policy.
//! - **NoPlan**: no gate.
//!
//! Denials return deterministic, instructive strings so the model can
//! navigate to the allowed alternative instead of flailing.

use super::r#trait::ToolCapability;
use crate::utils::plan_files::{self, PlanModeState};
use serde_json::Value;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Tools always allowed while Editing (either sub-state): the plan tool
/// itself (operation-level rules live inside it) and the question tool.
const EDITING_ALLOWED: &[&str] = &["plan", "follow_up_question"];

/// Names denied while Editing regardless of capability flags: channel
/// sends, browser mutators, and agent spawn/team mutators. Read-shaped
/// browser tools (navigate, content, screenshot, find, wait) stay
/// available for exploration in pre-init.
pub(crate) const EDITING_DENIED_NAMES: &[&str] = &[
    "telegram_send",
    "discord_send",
    "slack_send",
    "whatsapp_send",
    "trello_send",
    "a2a_send",
    "browser_click",
    "browser_type",
    "browser_eval",
    "spawn_agent",
    "send_input",
    "close_agent",
    "resume_agent",
];

/// Check a tool call against the session's plan-mode state. `None` means
/// the call proceeds (normal approval policy applies); `Some(reason)` is
/// a deterministic denial the model receives as the tool result.
pub(crate) async fn check_plan_gate(
    session_id: Uuid,
    tool_name: &str,
    capabilities: &[ToolCapability],
    input: &Value,
) -> Option<String> {
    let state = plan_files::plan_mode_state(session_id).await;
    let has = |cap: ToolCapability| capabilities.contains(&cap);

    match state {
        PlanModeState::NoPlan => None,

        PlanModeState::Active => {
            // Seed tool policy (locked): between user Approve and a
            // successful `start`, the design-track session may only read
            // and call the plan tool (add_tasks, start). Project writes,
            // bash, spawn, sends, and system mutators stay blocked so a
            // wandering seed turn cannot start editing the project before
            // the checklist exists.
            if crate::utils::plan_mode::in_seed_window(session_id).await {
                if EDITING_ALLOWED.contains(&tool_name) {
                    return None;
                }
                let mutator = has(ToolCapability::WriteFiles)
                    || has(ToolCapability::ExecuteShell)
                    || has(ToolCapability::SystemModification)
                    || EDITING_DENIED_NAMES.contains(&tool_name);
                if mutator {
                    return Some(format!(
                        "Plan gate: '{tool_name}' is blocked until the approved plan's \
                         checklist is seeded. Call `plan` add_tasks with the steps from \
                         the session .md, then `plan` start; project work begins after \
                         start succeeds."
                    ));
                }
                return None;
            }
            // Freeze the live design .md against generic write tools; the
            // checklist executes through the plan tool, not by rewriting
            // the approved design.
            if has(ToolCapability::WriteFiles) && write_targets_session_md(session_id, input).await
            {
                Some(
                    "Plan gate: the session plan .md is frozen while the checklist is \
                     Active. Execute tasks with the plan tool (start/complete); the \
                     design document is no longer editable."
                        .to_string(),
                )
            } else {
                None
            }
        }

        PlanModeState::PreInitEditing => {
            if EDITING_ALLOWED.contains(&tool_name) {
                return None;
            }
            if EDITING_DENIED_NAMES.contains(&tool_name) {
                return Some(format!(
                    "Plan gate: '{tool_name}' is blocked while the session is in Plan \
                     mode (pre-init Editing). Explore with reads, search, and bash, \
                     then call plan init to create the design document."
                ));
            }
            // Exploratory bash (and code execution) is explicitly allowed
            // pre-init, so the agent can investigate before `plan init`.
            if has(ToolCapability::ExecuteShell) {
                return None;
            }
            if has(ToolCapability::WriteFiles) {
                return Some(format!(
                    "Plan gate: project file writes are blocked while the session is \
                     in Plan mode (pre-init Editing): there is no plan document yet. \
                     Explore with reads, search, and bash, then call plan init; \
                     '{tool_name}' becomes relevant only after the plan is approved."
                ));
            }
            if has(ToolCapability::SystemModification) {
                return Some(format!(
                    "Plan gate: '{tool_name}' modifies system state and is blocked \
                     while the session is in Plan mode (pre-init Editing). Explore, \
                     then call plan init."
                ));
            }
            None
        }

        PlanModeState::PostInitEditing => {
            if EDITING_ALLOWED.contains(&tool_name) {
                return None;
            }
            if EDITING_DENIED_NAMES.contains(&tool_name) {
                return Some(format!(
                    "Plan gate: '{tool_name}' is blocked while the plan is being \
                     designed (Editing). Refine the session plan .md and wait for \
                     the user to approve the plan."
                ));
            }
            // All bash is denied post-init: the design phase writes prose,
            // not commands. (ExecuteShell first: code_exec carries
            // WriteFiles too and must not fall into the .md-write branch.)
            if has(ToolCapability::ExecuteShell) {
                return Some(
                    "Plan gate: bash is blocked while the plan is being designed \
                     (Editing). Exploration happened before plan init; now refine \
                     the session plan .md and wait for user approval. Execution \
                     starts after the plan is approved."
                        .to_string(),
                );
            }
            if has(ToolCapability::WriteFiles) {
                if write_targets_session_md(session_id, input).await {
                    return None;
                }
                let md = plan_files::plan_md_path(session_id).await;
                return Some(format!(
                    "Plan gate: while the plan is being designed (Editing), the ONLY \
                     writable file is the session plan document at {}. Write the \
                     design there; project files become editable after the user \
                     approves the plan.",
                    md.display()
                ));
            }
            if has(ToolCapability::SystemModification) {
                return Some(format!(
                    "Plan gate: '{tool_name}' modifies system state and is blocked \
                     while the plan is being designed (Editing). Refine the session \
                     plan .md and wait for user approval."
                ));
            }
            None
        }
    }
}

/// Whether a write-capable tool call targets the session plan `.md`.
///
/// The target is read from the conventional `path` / `file_path` input
/// keys. Absolute paths compare directly; relative paths are tried
/// against the OpenCrabs home (`write_opencrabs_file` semantics). A
/// write tool whose target cannot be determined does NOT match, so in
/// post-init Editing it is denied (safe default) and in Active it is
/// allowed (the freeze only guards the .md).
pub(crate) async fn write_targets_session_md(session_id: Uuid, input: &Value) -> bool {
    let Some(raw) = input
        .get("path")
        .or_else(|| input.get("file_path"))
        .and_then(|v| v.as_str())
    else {
        return false;
    };
    let md = plan_files::plan_md_path(session_id).await;
    let candidate = PathBuf::from(raw);
    if candidate.is_absolute() {
        paths_match(&candidate, &md)
    } else {
        paths_match(&crate::config::opencrabs_home().join(&candidate), &md)
    }
}

/// Path equality that tolerates symlinked parents (e.g. /var on macOS):
/// canonicalize both sides when possible, falling back to lexical
/// comparison for not-yet-existing files.
fn paths_match(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => a == b,
    }
}

/// Strip every write, shell, system-modifying, and Editing-denied tool from
/// `registry`, leaving a read-only surface (#649).
///
/// A subagent spawned while the parent session is in Plan-mode Editing must
/// not be a hole in the write-freeze: it runs under a fresh child session
/// that resolves to `NoPlan`, so [`check_plan_gate`] would not gate it. Rather
/// than thread the parent's Editing state through the child's every tool call,
/// we remove the mutating tools from the child registry outright — the child
/// can read, search, and analyze to inform or review the design, but cannot
/// write the project, run bash, or spawn further agents (which would let it
/// escape the freeze transitively). Removing the shell/system tools also drops
/// `spawn_agent` itself (it carries `SystemModification`), so a read-only child
/// cannot mint a non-restricted grandchild.
///
/// Mirrors the post-init Editing deny set (`ToolCapability::WriteFiles` /
/// `ExecuteShell` / `SystemModification` plus [`EDITING_DENIED_NAMES`]) so the
/// spawn-time filter and the per-call gate can never drift.
pub(crate) fn restrict_registry_to_read_only(registry: &super::ToolRegistry) {
    for name in registry.list_tools() {
        let denied_by_name = EDITING_DENIED_NAMES.contains(&name.as_str());
        let denied_by_cap = registry.get(&name).is_some_and(|t| {
            let caps = t.capabilities();
            caps.contains(&ToolCapability::WriteFiles)
                || caps.contains(&ToolCapability::ExecuteShell)
                || caps.contains(&ToolCapability::SystemModification)
        });
        if denied_by_name || denied_by_cap {
            registry.unregister(&name);
        }
    }
}
