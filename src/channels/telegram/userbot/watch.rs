//! Userbot watch loop — the read plane.
//!
//! Connects the persisted grammers session, streams updates, and forwards
//! text messages from `read`-granted chats through the bot handler. Replies exit
//! as the bot, so this loop never writes to Telegram as the user. Chats where
//! the bot is already a member should NOT be listed (double delivery); the
//! allowlist is for the chats only the user account can see.
//!
//! Own sends (`outgoing`) and bot-originated traffic are skipped, and the
//! loop exits on stream error — the ChannelManager sees the dead handle and
//! restarts it on the next reconcile, mirroring bot/WhatsApp agent recovery.

use std::sync::Arc;

use tokio::sync::Mutex;

use teloxide::prelude::Requester;

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

/// Does this chat carry `read` permission? Numeric id string match
/// against the `chat_permissions` map (negative ids = groups/channels,
/// positive = private chats).
fn chat_allowed(
    perms: &std::collections::BTreeMap<String, Vec<crate::config::types::ChatPermission>>,
    chat_id: i64,
) -> bool {
    perms
        .get(&chat_id.to_string())
        .is_some_and(|p| p.contains(&crate::config::types::ChatPermission::Read))
}

/// Chats carrying a `read` grant, as numeric ids — the ingestion set.
fn read_granted_chat_ids(
    perms: &std::collections::BTreeMap<String, Vec<crate::config::types::ChatPermission>>,
) -> Vec<i64> {
    perms
        .iter()
        .filter(|(_, p)| p.contains(&crate::config::types::ChatPermission::Read))
        .filter_map(|(k, _)| k.parse::<i64>().ok())
        .collect()
}

/// A bot token's leading `<user_id>:` segment — the bot's own user id.
fn bot_user_id_from_token(token: &str) -> Option<i64> {
    token.split(':').next()?.parse::<i64>().ok()
}

/// Probe each read-granted chat via the Bot API: if this bot is itself a
/// member, userbot ingestion double-delivers (bot handler + watch loop).
/// Warning only — any probe error is treated as "not a member" and never
/// blocks or delays the watch loop.
async fn warn_if_bot_member(bot: teloxide::Bot, bot_token: Arc<String>, chats: Vec<i64>) {
    let Some(bot_id) = bot_user_id_from_token(&bot_token) else {
        return;
    };
    for chat in chats {
        match bot
            .get_chat_member(
                teloxide::types::ChatId(chat),
                teloxide::types::UserId(bot_id as u64),
            )
            .await
        {
            Ok(member)
                if !matches!(
                    member.kind,
                    teloxide::types::ChatMemberKind::Left
                        | teloxide::types::ChatMemberKind::Banned { .. }
                ) =>
            {
                tracing::warn!(
                    "Telegram userbot: bot is also a member of read-granted chat {chat} — \
                     messages will be double-delivered (bot handler + ingestion). \
                     Remove the chat from chat_permissions if the bot already covers it."
                );
            }
            _ => {}
        }
    }
}

/// Spawn the watch loop. Returns the task handle for the ChannelManager.
pub(crate) async fn spawn(deps: UserbotDeps) -> anyhow::Result<tokio::task::JoinHandle<()>> {
    let cfg = deps.config_rx.borrow().clone();
    let ub = &cfg.channels.telegram.userbot;
    let permissions = ub.chat_permissions.clone();

    let (client, session, updates) = connect(ub).await?;
    if !client.is_authorized().await? {
        anyhow::bail!(
            "userbot session exists but is not authorized — run `opencrabs channel userbot-login`"
        );
    }
    let me = client.get_me().await?;
    tracing::info!(
        user = %me.first_name().unwrap_or_default(),
        chats = permissions.len(),
        "Telegram userbot watch loop starting (read-only)"
    );

    // Double-delivery diagnostic: fire-and-forget probe of read-granted
    // chats through the Bot API — warns if the bot is itself a member.
    let read_chats = read_granted_chat_ids(&permissions);
    if !read_chats.is_empty() {
        tokio::spawn(warn_if_bot_member(
            deps.bot.clone(),
            deps.bot_token.clone(),
            read_chats,
        ));
    }

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
        // Persist session drift (auth keys, peer cache, update state) on a
        // lazy ticker — a no-op unless the session was mutated.
        let mut save_ticker = tokio::time::interval(std::time::Duration::from_secs(60));
        save_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                update = stream.next() => { match update {
                Ok(Update::NewMessage(m)) => {
                    if m.outgoing() || m.via_bot_id().is_some() {
                        continue; // our own or bot-plane traffic: never loop it back
                    }
                    let Some(chat_id) = m.peer_id().bot_api_dialog_id() else {
                        continue;
                    };
                    if !chat_allowed(&permissions, chat_id) {
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
                    // FloodWait audit (steal-list task 8): no sleep here by
                    // design. The library's default AutoSleep already absorbs
                    // floods ≤60s in-process; larger floods landing here exit
                    // for restart and the ChannelManager's reconcile backs
                    // the loop off naturally. Tool-plane floods are
                    // structured instead — see tools::flood_wait_secs.
                    tracing::error!("Telegram userbot: stream error, exiting for restart: {e}");
                    break;
                }
                } },
                _ = save_ticker.tick() => {
                    if let Err(e) = session.save_if_dirty() {
                        tracing::warn!("Telegram userbot: session save failed: {e}");
                    }
                }
            }
        }
    }))
}

#[cfg(test)]
mod boot_warning_tests {
    use super::*;
    use crate::config::types::ChatPermission;

    fn perms() -> std::collections::BTreeMap<String, Vec<ChatPermission>> {
        let mut m = std::collections::BTreeMap::new();
        m.insert("-1001234567890".to_string(), vec![ChatPermission::Read]);
        m.insert(
            "-1009999999999".to_string(),
            vec![ChatPermission::Read, ChatPermission::Send],
        );
        m.insert("123456789".to_string(), vec![ChatPermission::Send]);
        m.insert("not-a-number".to_string(), vec![ChatPermission::Read]);
        m
    }

    #[test]
    fn token_prefix_decodes_bot_user_id() {
        assert_eq!(
            bot_user_id_from_token("8420472289:AAF-xyz_hash_material"),
            Some(8420472289)
        );
        assert_eq!(bot_user_id_from_token("no-colon-here"), None);
        assert_eq!(bot_user_id_from_token("abc:def"), None);
        assert_eq!(bot_user_id_from_token(""), None);
    }

    #[test]
    fn read_grant_selects_only_readable_numeric_chats() {
        let ids = read_granted_chat_ids(&perms());
        assert_eq!(ids, vec![-1001234567890, -1009999999999]);
    }
}
