//! Userbot watch loop — the read plane.
//!
//! Connects the persisted grammers session, streams updates, and forwards
//! text messages from `allowed_chats` through the bot handler. Replies exit
//! as the bot, so this loop never writes to Telegram as the user. Chats where
//! the bot is already a member should NOT be listed (double delivery); the
//! allowlist is for the chats only the user account can see.
//!
//! Own sends (`outgoing`) and bot-originated traffic are skipped, and the
//! loop exits on stream error — the ChannelManager sees the dead handle and
//! restarts it on the next reconcile, mirroring bot/WhatsApp agent recovery.

use std::sync::Arc;

use tokio::sync::Mutex;

use grammers_client::client::UpdatesConfiguration;
use grammers_client::update::Update;

use super::convert::to_bot_api;
use super::login::connect;
use crate::brain::agent::AgentService;
use crate::channels::telegram::TelegramState;
use crate::channels::telegram::handler::handle_message;
use crate::config::Config;
use crate::db::ChannelMessageRepository;

/// Everything `handle_message` needs — the same deps the bot dispatcher
/// passes, assembled once here and cloned into each message task.
pub(crate) struct UserbotDeps {
    pub bot: teloxide::Bot,
    pub agent: Arc<AgentService>,
    pub session_svc: crate::services::SessionService,
    pub bot_token: Arc<String>,
    pub shared_session: Arc<Mutex<Option<uuid::Uuid>>>,
    pub telegram_state: Arc<TelegramState>,
    pub config_rx: tokio::sync::watch::Receiver<Config>,
    pub channel_msg_repo: ChannelMessageRepository,
}

/// Is this chat id in the userbot allowlist? String compare to match the
/// config's Vec<String> (owners paste ids in either form; negative for
/// groups/channels, positive for private chats).
fn chat_allowed(allowed: &[String], chat_id: i64) -> bool {
    let id = chat_id.to_string();
    allowed.iter().any(|a| a.trim() == id)
}

/// Spawn the watch loop. Returns the task handle for the ChannelManager.
pub(crate) async fn spawn(deps: UserbotDeps) -> anyhow::Result<tokio::task::JoinHandle<()>> {
    let cfg = deps.config_rx.borrow().clone();
    let ub = &cfg.channels.telegram.userbot;
    let allowed = ub.allowed_chats.clone();

    let (client, _session, updates) = connect(ub).await?;
    if !client.is_authorized().await? {
        anyhow::bail!(
            "userbot session exists but is not authorized — run `opencrabs channel userbot-login`"
        );
    }
    let me = client.get_me().await?;
    tracing::info!(
        user = %me.first_name().unwrap_or_default(),
        chats = allowed.len(),
        "Telegram userbot watch loop starting (read-only)"
    );

    Ok(tokio::spawn(async move {
        let stream = match client
            .stream_updates(updates, UpdatesConfiguration::default())
            .await
        {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Telegram userbot: stream setup failed: {e}");
                return;
            }
        };
        let mut stream = stream;
        loop {
            match stream.next().await {
                Ok(Update::NewMessage(m)) => {
                    if m.outgoing() || m.via_bot_id().is_some() {
                        continue; // our own or bot-plane traffic: never loop it back
                    }
                    let Some(chat_id) = m.peer_id().bot_api_dialog_id() else {
                        continue;
                    };
                    if !chat_allowed(&allowed, chat_id) {
                        continue;
                    }
                    let Some(msg) = to_bot_api(&m) else {
                        continue;
                    };
                    let deps = UserbotDeps {
                        bot: deps.bot.clone(),
                        agent: deps.agent.clone(),
                        session_svc: deps.session_svc.clone(),
                        bot_token: deps.bot_token.clone(),
                        shared_session: deps.shared_session.clone(),
                        telegram_state: deps.telegram_state.clone(),
                        config_rx: deps.config_rx.clone(),
                        channel_msg_repo: deps.channel_msg_repo.clone(),
                    };
                    tokio::spawn(async move {
                        if let Err(e) = handle_message(
                            deps.bot,
                            msg,
                            deps.agent,
                            deps.session_svc,
                            deps.bot_token,
                            deps.shared_session,
                            deps.telegram_state,
                            deps.config_rx,
                            deps.channel_msg_repo,
                        )
                        .await
                        {
                            tracing::error!("Telegram userbot: handle_message error: {e}");
                        }
                    });
                }
                Ok(_) => {} // statuses, reads, typing: not forwarded
                Err(e) => {
                    tracing::error!("Telegram userbot: stream error, exiting for restart: {e}");
                    break;
                }
            }
        }
    }))
}
