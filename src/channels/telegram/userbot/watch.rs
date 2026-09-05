//! Receive-only MTProto update loop.
//!
//! Messages cross one boundary: allowlisted text becomes passive
//! `channel_messages` data. This loop never invokes the LLM and never sends,
//! edits, reacts, downloads, or calls arbitrary MTProto methods.

use grammers_client::client::UpdatesConfiguration;
use grammers_client::update::Update;

use super::capture::{chat_allowed, to_channel_message};
use super::login::connect;
use crate::config::types::TelegramUserbotConfig;
use crate::db::ChannelMessageRepository;

/// Spawn the independently restartable receive loop.
pub(crate) async fn spawn(
    config: TelegramUserbotConfig,
    messages: ChannelMessageRepository,
) -> anyhow::Result<tokio::task::JoinHandle<()>> {
    let allowed = config.allowed_chats.clone();
    let (client, session, updates, runner) = connect(&config).await?;
    if !client.is_authorized().await? {
        anyhow::bail!("userbot is not authorized — run `opencrabs channel userbot-login`");
    }
    let me = client.get_me().await?;
    tracing::info!(
        user = %me.first_name().unwrap_or_default(),
        chats = allowed.len(),
        "Telegram userbot passive capture starting (receive-only)"
    );

    Ok(tokio::spawn(async move {
        // `runner` moves into this task on purpose: when the manager aborts
        // the loop, or it breaks on a stream error, the guard drops and the
        // MTProto connection goes down with it instead of lingering beside
        // the next pool that reconcile opens on the same session file.
        let mut stream = match client
            .stream_updates(updates, UpdatesConfiguration::default())
            .await
        {
            Ok(stream) => stream,
            Err(error) => {
                tracing::error!("Telegram userbot stream setup failed: {error}");
                drop(runner);
                return;
            }
        };
        let mut save_tick = tokio::time::interval(std::time::Duration::from_secs(60));
        save_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                update = stream.next() => match update {
                    Ok(Update::NewMessage(message)) => {
                        let Some(chat_id) = message.peer_id().bot_api_dialog_id() else {
                            continue;
                        };
                        // Gate before conversion, logging, storage, or any other work.
                        if !chat_allowed(&allowed, chat_id) {
                            continue;
                        }
                        let Some(row) = to_channel_message(&message) else {
                            continue;
                        };
                        if let Err(error) = messages.insert(&row).await {
                            tracing::warn!(
                                chat_id,
                                message_id = message.id(),
                                "Telegram userbot passive capture failed: {error}"
                            );
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        tracing::error!(
                            "Telegram userbot stream failed; exiting for reconcile restart: {error}"
                        );
                        break;
                    }
                },
                _ = save_tick.tick() => {
                    if let Err(error) = session.save_if_dirty() {
                        tracing::warn!("Telegram userbot session save failed: {error}");
                    }
                }
            }
        }
        drop(runner);
    }))
}
