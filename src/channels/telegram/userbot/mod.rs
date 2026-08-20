//! Telegram userbot — MTProto user session (grammers) as a read-plane
//! companion to the Bot API bot.
//!
//! Login is driven by `opencrabs channel userbot-login` (QR by default,
//! `--code` for the phone-code path). The watch loop lives in
//! [`watch`] and forwards `allowed_chats` messages through the same bot
//! handler; see the map at `~/.opencrabs/research/telegram-userbot-map.md`
//! for the verified grammers 0.10 wiring notes.

pub(crate) mod chat_login;
pub(crate) mod convert;
pub(crate) mod login;
pub(crate) mod session;
pub(crate) mod setup;
pub(crate) mod tools;
pub(crate) mod watch;

use std::path::PathBuf;

use crate::config::opencrabs_home;
use crate::config::types::TelegramUserbotConfig;

/// Where the userbot session lives (JSON; see [`session::FileSession`]).
///
/// The session file IS the logged-in account: anyone holding it can act as
/// the user without any further auth. It belongs next to keys.toml in the
/// OpenCrabs home — never in a project dir, never in tmp, never committed.
pub(crate) fn session_file(cfg: &TelegramUserbotConfig) -> PathBuf {
    match cfg.session_path.as_deref() {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => opencrabs_home().join("telegram_userbot.session.json"),
    }
}

/// MTProto app credentials (my.telegram.org → API development tools).
/// `api_hash` is a secret — it arrives via keys.toml and is never logged.
pub(crate) struct UserbotCreds {
    pub api_id: i32,
    pub api_hash: String,
    pub phone: String,
}

pub(crate) fn resolve_creds(cfg: &TelegramUserbotConfig) -> anyhow::Result<UserbotCreds> {
    let api_id = cfg.api_id.ok_or_else(|| {
        anyhow::anyhow!(
            "channels.telegram.userbot.api_id missing — get it from my.telegram.org → API development tools"
        )
    })?;
    let api_hash = cfg
        .api_hash
        .clone()
        .filter(|h| !h.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("channels.telegram.userbot.api_hash missing (set it in keys.toml)")
        })?;
    let phone = cfg
        .phone
        .clone()
        .filter(|p| !p.is_empty())
        .ok_or_else(|| anyhow::anyhow!("channels.telegram.userbot.phone missing"))?;
    Ok(UserbotCreds {
        api_id: api_id as i32,
        api_hash,
        phone,
    })
}
