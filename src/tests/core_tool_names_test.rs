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
use crate::brain::tools::catalog::{CORE_TOOLS, EXTENDED_TOOL_INVENTORY, is_core};
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
fn extended_inventory_lists_team_and_brave_and_drops_stale_provider_vision() {
    // #670: the lazy-mode roster was missing the team tools and brave_search,
    // and carried a stale `provider_vision` (the real name is analyze_image,
    // which is core).
    let names: Vec<&str> = EXTENDED_TOOL_INVENTORY
        .iter()
        .flat_map(|(_, names)| names.iter().copied())
        .collect();
    for t in [
        "team_create",
        "team_delete",
        "team_broadcast",
        "brave_search",
    ] {
        assert!(
            names.contains(&t),
            "extended inventory missing `{t}` (#670)"
        );
    }
    assert!(
        !names.contains(&"provider_vision"),
        "stale `provider_vision` still in the inventory (#670)"
    );
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
