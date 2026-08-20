//! Userbot tool plane — interactive on-demand commands over the MTProto
//! user session (the gap PR #1113 never built; see
//! `~/.opencrabs/research/pr1113-vs-mcp-telegram-gap-analysis.md`).
//!
//! The ingestion plane (watch loop) is ambient and read-only. This module
//! is the complementary interactive layer: read/search a chat, global
//! search, send/edit as the user, discovery, raw MTProto — 8 tools.
//!
//! Governance is data, not ceremony: outbound tools (send/edit) require
//! the target chat in `TelegramUserbotConfig::outbound_allowlist` (empty
//! = strictly read-only), and raw MTProto requires an explicit
//! per-invocation `confirm` flag. Params travel via file (see
//! [`params`]) so complex filters never fight shell quoting.

pub(crate) mod client;
pub(crate) mod commands;
pub(crate) mod dispatch;
pub(crate) mod mapping;
pub(crate) mod params;
pub(crate) mod raw;
pub(crate) mod render;
pub(crate) mod transport;

use anyhow::{Context, Result};
use serde_json::Value;

/// CLI entrypoint: `opencrabs userbot tool --params-file <path>`.
///
/// One process per invocation: load the invocation, authorize it
/// against config, connect the session, execute, print the envelope.
/// Denials exit non-zero with the refusal reason — outbound targets
/// not in `outbound_allowlist` and unconfirmed raw calls stop here,
/// before any network touch.
#[cfg(feature = "telegram-userbot")]
pub(crate) async fn cmd_userbot_tool(
    config: &crate::config::Config,
    params_file: &str,
) -> Result<()> {
    let cfg = &config.channels.telegram.userbot;
    let invocation = params::ToolInvocation::load(std::path::Path::new(params_file))
        .with_context(|| format!("loading params file {params_file}"))?;
    if let Err(denial) = dispatch::authorize(&invocation.command, cfg) {
        anyhow::bail!("refused: {denial}");
    }
    let tool = client::connect(cfg).await?;
    let value: Value = dispatch::run(&invocation.command, &tool.client).await?;
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}
