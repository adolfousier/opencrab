//! Telegram userbot — local MTProto authentication and session persistence.
//!
//! The receive loop is added separately so this commit remains independently
//! buildable and reviewable.

pub(crate) mod login;
pub(crate) mod session;

use std::path::PathBuf;

use crate::config::opencrabs_home;
use crate::config::types::TelegramUserbotConfig;

/// Where the local userbot session lives.
pub(crate) fn session_file(cfg: &TelegramUserbotConfig) -> PathBuf {
    match cfg.session_path.as_deref() {
        Some(path) if !path.is_empty() => PathBuf::from(path),
        _ => opencrabs_home().join("telegram_userbot.session.json"),
    }
}

pub(crate) struct UserbotCreds {
    pub api_id: i32,
    pub api_hash: String,
    pub phone: String,
}

pub(crate) fn resolve_creds(cfg: &TelegramUserbotConfig) -> anyhow::Result<UserbotCreds> {
    let api_id = cfg.api_id.ok_or_else(|| {
        anyhow::anyhow!("channels.telegram.userbot.api_id missing — get it from my.telegram.org")
    })?;
    let api_hash = cfg
        .api_hash
        .clone()
        .filter(|hash| !hash.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("channels.telegram.userbot.api_hash missing in keys.toml")
        })?;
    let phone = cfg
        .phone
        .clone()
        .filter(|phone| !phone.is_empty())
        .ok_or_else(|| anyhow::anyhow!("channels.telegram.userbot.phone missing"))?;
    Ok(UserbotCreds {
        api_id: api_id as i32,
        api_hash,
        phone,
    })
}
