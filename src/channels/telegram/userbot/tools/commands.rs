//! The tool commands — plain serde data, no behavior.
//!
//! Mirrors the MCP package's tool surface (moneyacademyke parity target)
//! grounded against vendored grammers 0.10 signatures
//! (`~/.opencrabs/research/gramers-0.10-signatures.txt`):
//!
//! | Tool | gramers surface |
//! |---|---|---|
//! | `read_chat` | dialogs iter + messages by filter |
//! | `search_chat` | messages iter + text filter |
//! | `search_global` | `resolve_username` + chat search |
//! | `send_message` | `send_message` (peer ref) |
//! | `send_file` | media upload path |
//! | `edit_message` | `edit_message` |
//! | `react` | `send_reactions` (emoticon string) |
//! | `discover` | `iter_dialogs`, folders, contacts |
//! | `raw` | typed TL invoke |
//!
//! `class` is the governance data: reads are always allowed (the session
//! exists to read), outbound requires `send` for the target in `chat_permissions`,
//! raw needs an explicit confirm on the invocation itself.
//!
//! Autonomy classes: read = reversible, outbound = irreversible
//! (delivered as the user, cannot be unsent), raw = irreversible and
//! unbounded. The CLI (task 6) routes outbound through OpenCrabs'
//! approval policy on top of the allowlist — no silent sends.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case", tag = "tool")]
pub(crate) enum ToolCommand {
    ReadChat(ReadChat),
    SearchChat(SearchChat),
    SearchGlobal(SearchGlobal),
    SendMessage(SendMessage),
    SendFile(SendFile),
    SendToPhone(SendToPhone),
    EditMessage(EditMessage),
    React(React),
    Download(Download),
    Discover(Discover),
    Raw(Raw),
}

impl ToolCommand {
    /// Governance classification — see the module doc.
    pub(crate) fn class(&self) -> ToolClass<'_> {
        match self {
            ToolCommand::ReadChat(_) | ToolCommand::SearchChat(_) => ToolClass::Read,
            ToolCommand::SearchGlobal(_) | ToolCommand::Discover(_) => ToolClass::Read,
            ToolCommand::Download(_) => ToolClass::Read,
            ToolCommand::SendMessage(s) => ToolClass::Outbound(&s.chat),
            ToolCommand::SendFile(s) => ToolClass::Outbound(&s.chat),
            ToolCommand::SendToPhone(s) => ToolClass::Outbound(&s.phone),
            ToolCommand::EditMessage(e) => ToolClass::Outbound(&e.chat),
            ToolCommand::React(r) => ToolClass::Outbound(&r.chat),
            ToolCommand::Raw(_) => ToolClass::Dangerous,
        }
    }

    /// Stable tool name (params-file `tool` tag). Test-only: the production
    /// path reads the tag via serde, so this exists for assertions.
    #[cfg(test)]
    pub(crate) fn name(&self) -> &'static str {
        match self {
            ToolCommand::ReadChat(_) => "read_chat",
            ToolCommand::SearchChat(_) => "search_chat",
            ToolCommand::SearchGlobal(_) => "search_global",
            ToolCommand::SendMessage(_) => "send_message",
            ToolCommand::SendFile(_) => "send_file",
            ToolCommand::SendToPhone(_) => "send_to_phone",
            ToolCommand::EditMessage(_) => "edit_message",
            ToolCommand::React(_) => "react",
            ToolCommand::Download(_) => "download",
            ToolCommand::Discover(_) => "discover",
            ToolCommand::Raw(_) => "raw",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ToolClass<'a> {
    /// Always allowed while a session exists.
    Read,
    /// Allowed only when the target carries `send` in `chat_permissions`.
    Outbound(&'a str),
    /// Allowed only with explicit per-invocation confirmation.
    Dangerous,
}

/// Read messages from one chat. MCP parity: date/sender/thread filters.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct ReadChat {
    pub chat: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<i64>,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

/// Text search within one chat.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct SearchChat {
    pub chat: String,
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

/// Search across all dialogs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct SearchGlobal {
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

/// Send a text message as the user. Outbound: visible, spam-scored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct SendMessage {
    pub chat: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<i64>,
    /// Unix seconds when Telegram should send this message. Must be
    /// in the future and within Telegram's ~366-day scheduling window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule_unix: Option<i64>,
    /// After sending, poll up to this many seconds (capped at 120)
    /// for the first incoming reply — sender ≠ me, id > sent id.
    /// Timeout is not an error: the envelope reports `reply: null`
    /// plus `waited_secs`. Process-local polling; no daemon state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_reply_secs: Option<u64>,
}

/// Send a file as the user. Outbound: visible, spam-scored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct SendFile {
    pub chat: String,
    /// Local path to the file to upload.
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
}

/// Send a text to a phone number by importing a temporary contact
/// (`contacts.importContacts`), then sending to the imported peer.
/// Outbound: the allowlist target is the phone literal (E.164).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct SendToPhone {
    pub phone: String,
    pub text: String,
}

/// Edit a message the user previously sent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct EditMessage {
    pub chat: String,
    pub message_id: i64,
    pub new_text: String,
}

/// React to a message as the user (emoticon string, e.g. "👍").
/// Outbound: a reaction is a public write, same class as a send.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct React {
    pub chat: String,
    pub message_id: i64,
    /// Emoticon literal; empty string removes the reaction
    /// (`InputReactions::remove()` semantics via empty payload).
    pub emoji: String,
}

/// Download media attached to one message. Read-class on the Telegram
/// side: a fetch — the only write is the local file, and explicit paths
/// are validated (absolute, under $HOME, no `..`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct Download {
    /// Chat containing the message (id, @username, or "me").
    pub chat: String,
    pub message_id: i64,
    /// Explicit save path — must be absolute, under $HOME, without `..`
    /// components. Default: ~/.opencrabs/userbot_media/<chat>/msg-<id>.bin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// Discover chats/users: dialogs, folders, contacts by phone.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct Discover {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

/// Raw typed MTProto invoke. Dangerous: bypasses every typed guard.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct Raw {
    /// The raw RPC constructor name (e.g. `messages.GetHistory`).
    pub method: String,
    /// JSON params for the constructor payload.
    pub params: serde_json::Value,
    /// Explicit confirmation on the invocation itself — defaults false.
    #[serde(default)]
    pub confirm: bool,
}

pub(crate) const DEFAULT_LIMIT: u32 = 20;

const fn default_limit() -> u32 {
    DEFAULT_LIMIT
}
