//! Userbot tool plane — interactive on-demand commands over the MTProto
//! user session (the gap PR #1113 never built; see
//! `~/.opencrabs/research/pr1113-vs-mcp-telegram-gap-analysis.md`).
//!
//! The ingestion plane (watch loop) is ambient and read-only. This module
//! is the complementary interactive layer: read/search a chat, global
//! search, send/edit as the user, discovery, raw MTProto — 8 tools.
//!
//! Governance is data, not ceremony: outbound tools (send/edit) require
//! the target chat in `TelegramUserbotConfig::chat_permissions` (no `send` grant
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
/// without `send` in `chat_permissions` and unconfirmed raw calls stop here,
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
    let tool = match client::connect(cfg).await {
        Ok(t) => t,
        Err(e) => return flood_exit(e),
    };
    let value: Value = match dispatch::run(&invocation.command, &tool.client).await {
        Ok(v) => v,
        Err(e) => return flood_exit(e),
    };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

/// FLOOD_WAIT (RPC 420) extraction from a tool error chain. The library's
/// default `AutoSleep` already sleeps floods ≤60s once in-process; larger
/// waits propagate to the caller. The wait IS the tool's answer — surface
/// it as a structured envelope (exit 0) so the caller re-invokes after N
/// seconds instead of blind-retrying into another flood.
pub(crate) fn flood_wait_secs(err: &anyhow::Error) -> Option<u64> {
    use grammers_client::InvocationError;
    for cause in err.chain() {
        if let Some(InvocationError::Rpc(rpc)) = cause.downcast_ref::<InvocationError>()
            && rpc.code == 420
        {
            return rpc.value.map(u64::from);
        }
    }
    None
}

/// On flood: print the structured wait envelope and exit 0 (the wait is a
/// valid answer, not a failure). Any other error propagates untouched.
fn flood_exit(err: anyhow::Error) -> Result<()> {
    match flood_wait_secs(&err) {
        Some(secs) => {
            println!(
                "{}",
                serde_json::json!({ "error": "flood_wait", "retry_after_secs": secs })
            );
            Ok(())
        }
        None => Err(err),
    }
}
