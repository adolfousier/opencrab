//! Core tool names must match the tools' real `name()` (#669).
//!
//! `CORE_TOOLS` held aliases (`context` / `http_client` / `config_tool`) that
//! did not match the tools' real `name()` (`session_context` / `http_request` /
//! `config_manager`). `is_core()` is an exact match, so in lazy mode
//! `get_tool_definitions_filtered` dropped these always-on tools from the
//! default set — the agent was told it had `config_manager` but never got its
//! schema. This resolves each name against the real `Tool::name()` so the drift
//! can't come back.

use crate::brain::tools::Tool;
use crate::brain::tools::catalog::{CORE_TOOLS, is_core};
use crate::brain::tools::config_tool::ConfigTool;
use crate::brain::tools::context::ContextTool;
use crate::brain::tools::http::HttpClientTool;

#[test]
fn previously_drifted_core_tools_resolve_to_their_real_names() {
    for name in [ContextTool.name(), HttpClientTool.name(), ConfigTool.name()] {
        assert!(
            CORE_TOOLS.contains(&name),
            "core tool `{name}` missing from CORE_TOOLS"
        );
        assert!(is_core(name), "is_core(`{name}`) must be true");
    }
}

#[test]
fn old_drifted_aliases_are_gone() {
    // If any alias comes back it silently masks the real tool again.
    for alias in ["context", "http_client", "config_tool"] {
        assert!(
            !CORE_TOOLS.contains(&alias),
            "drifted alias `{alias}` is back in CORE_TOOLS"
        );
        assert!(!is_core(alias), "is_core(`{alias}`) must be false");
    }
}
