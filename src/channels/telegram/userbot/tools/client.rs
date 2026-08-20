//! One-shot client bootstrap for tool invocations.
//!
//! Tools reuse the session file earned by `userbot-login`. The daemon's
//! watch loop may hold the same file concurrently; grammers re-saves state
//! on connect/updates, and a racing save costs the loop one startup
//! re-sync (reconcile handles it), never a re-login. Short-lived tool
//! processes make this benign; revisit only if state thrash shows up.

use std::sync::Arc;

use anyhow::Result;
use grammers_client::Client;
use grammers_session::updates::UpdatesLike;
use tokio::sync::mpsc;

use crate::channels::telegram::userbot::login;
use crate::channels::telegram::userbot::session::FileSession;
use crate::config::types::TelegramUserbotConfig;

/// A connected client for one tool invocation.
pub(crate) struct ToolClient {
    pub client: Client,
    /// Held, not read: whether `Client` retains its own `Arc<FileSession>`
    /// is unverified in grammers 0.10, and the session file must outlive any
    /// state re-save. Costs nothing to hold for a one-shot process.
    _session: Arc<FileSession>,
    /// Held, not read: dropping the update receiver while the pool runner is
    /// alive has unverified semantics in grammers 0.10, so it lives as long
    /// as this struct and the process owning it. The daemon's watch loop is
    /// the only update consumer that matters.
    _updates: mpsc::UnboundedReceiver<UpdatesLike>,
}

/// Connect on the configured session for a one-shot tool call.
pub(crate) async fn connect(cfg: &TelegramUserbotConfig) -> Result<ToolClient> {
    let (client, session, updates) = login::connect(cfg).await?;
    Ok(ToolClient {
        client,
        _session: session,
        _updates: updates,
    })
}
