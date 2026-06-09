//! Tool tiering for lazy schema loading.
//!
//! Every agent turn used to ship ALL ~95 registered tool schemas (~20k tokens)
//! to the provider, even for a "reply yes" turn — they're counted in
//! `input_tokens` on every request. This module splits tools into a small
//! always-injected CORE set and an EXTENDED set the agent pulls on demand via
//! the `tool_search` tool (mirroring how contextual brain files are loaded via
//! `load_brain_file`).
//!
//! When `[agent] lazy_tools` is on, a request carries: CORE ∪ `tool_search` ∪
//! whatever EXTENDED tools the session has activated via `tool_search`. When
//! off, behaviour is unchanged (all tools every request).

/// The discovery tool's name — always injected alongside the core set so the
/// agent can always reach for additional tools.
pub const TOOL_SEARCH_NAME: &str = "tool_search";

/// Fundamental tools injected on EVERY request. Chosen to cover almost any
/// task without a discovery round-trip: file I/O, shell, the common searches,
/// core workflow/orchestration, the brain-file loader, and config/session
/// basics. Everything else is reached through `tool_search`.
pub const CORE_TOOLS: &[&str] = &[
    // File I/O + shell — the bread and butter
    "read_file",
    "write_file",
    "edit_file",
    "hashline_edit",
    "bash",
    "ls",
    "glob",
    "grep",
    // Search — hit constantly
    "web_search",
    "exa_search",
    "memory_search",
    // Workflow / orchestration
    "task",
    "context",
    "plan",
    "http_client",
    // Loaders + system basics
    "load_brain_file",
    "write_opencrabs_file",
    "config_tool",
    "slash_command",
    "rename_session",
    "follow_up_question",
    // The discovery tool itself
    TOOL_SEARCH_NAME,
];

/// True when `name` is part of the always-injected core set (or the discovery
/// tool). Core tools are never gated behind `tool_search`.
pub fn is_core(name: &str) -> bool {
    CORE_TOOLS.contains(&name)
}

/// Coarse category for an EXTENDED tool, derived from its name. Used to group
/// `tool_search` results and to catalog the extended set in TOOLS.md. Returns
/// `"core"` for core tools.
pub fn tool_category(name: &str) -> &'static str {
    if is_core(name) {
        return "core";
    }
    match name {
        n if n.starts_with("browser_") => "browser",
        n if n.starts_with("telegram_")
            || n.starts_with("whatsapp_")
            || n.starts_with("discord_")
            || n.starts_with("slack_")
            || n.starts_with("trello_") =>
        {
            "channels"
        }
        n if n.starts_with("spawn_agent")
            || n.starts_with("wait_agent")
            || n.starts_with("send_input")
            || n.starts_with("close_agent")
            || n.starts_with("resume_agent")
            || n.starts_with("team_") =>
        {
            "agents"
        }
        n if n.starts_with("analyze_image")
            || n.starts_with("analyze_video")
            || n.starts_with("generate_image")
            || n.starts_with("provider_vision") =>
        {
            "media"
        }
        n if n.starts_with("feedback_")
            || n == "self_improve"
            || n == "rebuild"
            || n == "evolve"
            || n == "tool_manage"
            || n == "rsi_proposals" =>
        {
            "system"
        }
        n if n == "cron_manage"
            || n == "session_search"
            || n == "channel_search"
            || n == "a2a_send" =>
        {
            "utility"
        }
        _ => "other",
    }
}
