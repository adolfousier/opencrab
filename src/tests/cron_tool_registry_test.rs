//! Regression for the multi-profile cron daemon running its jobs toolless.
//!
//! The daemon built its own `ChannelFactory` but never wired a tool registry,
//! so every cron agent got an EMPTY registry and each tool call failed with
//! "Tool not found: bash". The fix routes both the interactive startup and the
//! daemon through `cli::tool_setup::register_core_agent_tools`. This pins that
//! the shared helper actually POPULATES the registry with the core tools cron
//! jobs use — catching an empty/regressed registry without spinning up a
//! daemon. (The daemon then wires the result via `factory.set_tool_registry`.)

use crate::brain::tools::registry::ToolRegistry;
use crate::cli::tool_setup::register_core_agent_tools;
use crate::config::Config;
use crate::db::Database;
use std::sync::Arc;

#[tokio::test]
async fn register_core_agent_tools_populates_registry_with_cron_tools() {
    let db = Database::connect_in_memory().await.expect("in-memory db");
    db.run_migrations().await.expect("migrations");
    let config = Config::default();

    let registry = Arc::new(ToolRegistry::new());
    register_core_agent_tools(&registry, &db, &config);

    assert!(
        registry.count() > 0,
        "registry must not be empty — an empty registry is exactly the daemon bug"
    );

    // The concrete tools that failed with 'Tool not found' in the daemon log,
    // plus the rest of the core set cron jobs rely on.
    for name in [
        "bash",
        "read_file",
        "write_file",
        "edit_file",
        "ls",
        "glob",
        "grep",
        "execute_code",
        "session_search",
        "cron_manage",
        "plan",
        "web_search",
        "memory_search",
        "config_manager",
        "tool_search",
    ] {
        assert!(
            registry.has_tool(name),
            "core tool '{name}' must be registered by register_core_agent_tools"
        );
    }
}
