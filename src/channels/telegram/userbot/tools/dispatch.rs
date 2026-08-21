//! Dispatch: authorize first, execute second.
//!
//! `authorize` is pure and fully implemented — it is the governance gate
//! the MCP package never had (their `tg_send_message` ran at a 45%
//! failure rate with no local allowlist). `run` executes authorized
//! commands against a live client: the four read tools and all four
//! outbound tools (send/file/phone/edit) are wired to the gramers
//! transport; raw MTProto (task 5) refuses with a pointer until it
//! lands. Outbound calls assume `authorize` passed — execution without
//! consent is a caller bug, not a runtime state.

use anyhow::Result;
use grammers_client::Client;
use serde_json::Value;

use super::commands::{ToolClass, ToolCommand};
use super::raw;
use super::transport;
use crate::config::types::TelegramUserbotConfig;

/// Why a tool call was refused. Data, so callers (and tests) can match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Denial {
    /// The userbot is disabled — no session exists to serve any tool.
    Disabled,
    /// Outbound target's `chat_permissions` entry lacks `send` (or the
    /// chat is absent from the map entirely).
    OutboundNotAllowed { target: String },
    /// Raw MTProto requires `confirm: true` on the invocation.
    RawUnconfirmed,
}

impl std::fmt::Display for Denial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Denial::Disabled => write!(f, "userbot disabled"),
            Denial::OutboundNotAllowed { target } => write!(
                f,
                "outbound target {target} lacks `send` in channels.telegram.userbot.chat_permissions"
            ),
            Denial::RawUnconfirmed => {
                write!(f, "raw MTProto requires confirm: true on the invocation")
            }
        }
    }
}

/// The governance gate: pure function of (command, config).
///
/// - Read tools pass whenever the userbot is enabled.
/// - Outbound tools additionally require `send` for the target chat in
///   `chat_permissions` (no `send` grants = strictly read-only, by design).
/// - Raw requires its own `confirm` flag — no config default lets it
///   through silently.
pub(crate) fn authorize(cmd: &ToolCommand, cfg: &TelegramUserbotConfig) -> Result<(), Denial> {
    if !cfg.enabled {
        return Err(Denial::Disabled);
    }
    match cmd.class() {
        ToolClass::Read => Ok(()),
        ToolClass::Outbound(target) => {
            if cfg.may_send(target) {
                Ok(())
            } else {
                Err(Denial::OutboundNotAllowed {
                    target: target.to_string(),
                })
            }
        }
        // confirm lives on the Raw command itself (per-invocation).
        ToolClass::Dangerous => match cmd {
            ToolCommand::Raw(raw) if raw.confirm => Ok(()),
            _ => Err(Denial::RawUnconfirmed),
        },
    }
}

/// Execute an authorized command against a live grammers client.
/// The caller (CLI, task 6) owns the client lifecycle and is expected
/// to run `authorize` first — execution assumes consent.
pub(crate) async fn run(cmd: &ToolCommand, client: &Client) -> Result<Value> {
    match cmd {
        ToolCommand::ReadChat(c) => transport::read_chat(client, c).await,
        ToolCommand::SearchChat(c) => transport::search_chat(client, c).await,
        ToolCommand::SearchGlobal(c) => transport::search_global(client, c).await,
        ToolCommand::Discover(c) => transport::discover(client, c).await,
        ToolCommand::SendMessage(c) => transport::send_text(client, c).await,
        ToolCommand::SendFile(c) => transport::send_document(client, c).await,
        ToolCommand::SendToPhone(c) => transport::send_phone(client, c).await,
        ToolCommand::EditMessage(c) => transport::edit_text(client, c).await,
        ToolCommand::React(c) => transport::react(client, c).await,
        ToolCommand::Download(c) => transport::download(client, c).await,
        ToolCommand::Raw(c) => raw::run_raw(client, c).await,
    }
}
