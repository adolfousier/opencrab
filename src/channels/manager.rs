//! Channel Manager
//!
//! Manages the lifecycle of channel agents (Telegram, WhatsApp, Discord, Slack, Trello).
//! Spawns and stops channels dynamically when the config changes at runtime,
//! so that toggling `channels.*.enabled` in config.toml takes effect without restart.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::task::JoinHandle;

use crate::channels::ChannelFactory;
use crate::config::Config;

/// What reconcile should do with a channel this pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChannelAction {
    /// Not running (or its agent task died) and it should be: (re)start it.
    Start,
    /// Running but it should not be: stop it.
    Stop,
    /// Desired state already matches: do nothing.
    Noop,
}

/// Decide what to do with a channel from its desired state and whether its
/// agent task is still ALIVE.
///
/// The key case (issues #239/#240): when a channel agent crashes or exits, its
/// `JoinHandle` lingers in the map. Keying "running" off `contains_key` then
/// treats a dead agent as running, so an enabled channel is never restarted and
/// the WhatsApp pairing QR never reappears. Treating a finished handle as
/// not-alive yields `Start`, so a dead agent auto-restarts.
pub(crate) fn channel_action(should_run: bool, alive: bool) -> ChannelAction {
    match (should_run, alive) {
        (true, false) => ChannelAction::Start,
        (false, true) => ChannelAction::Stop,
        _ => ChannelAction::Noop,
    }
}

/// A channel counts as running only while its agent task is still alive; a
/// finished handle is stale (the agent exited) and must not block a restart.
fn handle_alive(handles: &HashMap<String, JoinHandle<()>>, name: &str) -> bool {
    handles.get(name).is_some_and(|h| !h.is_finished())
}

/// Manages running channel agents, allowing dynamic spawn/stop on config reload.
pub struct ChannelManager {
    handles: tokio::sync::Mutex<HashMap<String, JoinHandle<()>>>,
    channel_factory: Arc<ChannelFactory>,
    db_pool: deadpool_sqlite::Pool,

    #[cfg(feature = "telegram")]
    telegram_state: Arc<crate::channels::telegram::TelegramState>,
    #[cfg(feature = "whatsapp")]
    whatsapp_state: Arc<crate::channels::whatsapp::WhatsAppState>,
    #[cfg(feature = "discord")]
    discord_state: Arc<crate::channels::discord::DiscordState>,
    #[cfg(feature = "slack")]
    slack_state: Arc<crate::channels::slack::SlackState>,
    #[cfg(feature = "trello")]
    trello_state: Arc<crate::channels::trello::TrelloState>,
}

impl ChannelManager {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        channel_factory: Arc<ChannelFactory>,
        db_pool: deadpool_sqlite::Pool,
        #[cfg(feature = "telegram")] telegram_state: Arc<crate::channels::telegram::TelegramState>,
        #[cfg(feature = "whatsapp")] whatsapp_state: Arc<crate::channels::whatsapp::WhatsAppState>,
        #[cfg(feature = "discord")] discord_state: Arc<crate::channels::discord::DiscordState>,
        #[cfg(feature = "slack")] slack_state: Arc<crate::channels::slack::SlackState>,
        #[cfg(feature = "trello")] trello_state: Arc<crate::channels::trello::TrelloState>,
    ) -> Self {
        Self {
            handles: tokio::sync::Mutex::new(HashMap::new()),
            channel_factory,
            db_pool,
            #[cfg(feature = "telegram")]
            telegram_state,
            #[cfg(feature = "whatsapp")]
            whatsapp_state,
            #[cfg(feature = "discord")]
            discord_state,
            #[cfg(feature = "slack")]
            slack_state,
            #[cfg(feature = "trello")]
            trello_state,
        }
    }

    /// Compare running channels against config and spawn/stop as needed.
    pub async fn reconcile(&self, config: &Config) {
        let mut handles = self.handles.lock().await;

        #[cfg(feature = "telegram")]
        self.reconcile_telegram(config, &mut handles).await;

        #[cfg(feature = "telegram-userbot")]
        self.reconcile_telegram_userbot(config, &mut handles).await;

        #[cfg(feature = "whatsapp")]
        self.reconcile_whatsapp(config, &mut handles).await;

        #[cfg(feature = "discord")]
        self.reconcile_discord(config, &mut handles).await;

        #[cfg(feature = "slack")]
        self.reconcile_slack(config, &mut handles).await;

        #[cfg(feature = "trello")]
        self.reconcile_trello(config, &mut handles).await;
    }

    #[cfg(feature = "telegram")]
    async fn reconcile_telegram(
        &self,
        config: &Config,
        handles: &mut HashMap<String, JoinHandle<()>>,
    ) {
        let tg = &config.channels.telegram;
        let has_valid_token = tg
            .token
            .as_ref()
            .map(|t| {
                if t.is_empty() || !t.contains(':') {
                    return false;
                }
                let parts: Vec<&str> = t.splitn(2, ':').collect();
                parts.len() == 2 && parts[0].parse::<u64>().is_ok() && parts[1].len() >= 30
            })
            .unwrap_or(false);

        let should_run = tg.enabled && has_valid_token;
        match channel_action(should_run, handle_alive(handles, "telegram")) {
            ChannelAction::Start => {
                if let Some(ref token) = tg.token {
                    let token_hash = crate::config::profile::hash_token(token);
                    if let Err(e) =
                        crate::config::profile::acquire_token_lock("telegram", &token_hash)
                    {
                        tracing::error!(
                            "ChannelManager: Telegram token lock denied, this channel will NOT run: {}. \
                             Another profile is using the same bot credential; give each profile its own.",
                            e
                        );
                        return;
                    }
                    // Wire the reaction queue so a mid-turn reaction is injected
                    // into the running loop (#302 Stage 2).
                    let reaction_cb = self.telegram_state.reaction_queue_callback();
                    // Background-task resume producer (#722): filled with a weak
                    // ref to the agent after it's built (can't capture it at
                    // creation), so a finished detached command resumes the chat.
                    let agent_holder: std::sync::Arc<
                        std::sync::Mutex<
                            Option<std::sync::Weak<crate::brain::agent::AgentService>>,
                        >,
                    > = std::sync::Arc::new(std::sync::Mutex::new(None));
                    let enqueue_cb = crate::channels::telegram::resume::build_enqueue_callback(
                        self.telegram_state.clone(),
                        agent_holder.clone(),
                    );
                    let tg_agent_service = self
                        .channel_factory
                        .create_agent_service_full(Some(reaction_cb), Some(enqueue_cb))
                        .await;
                    if let Ok(mut h) = agent_holder.lock() {
                        *h = Some(std::sync::Arc::downgrade(&tg_agent_service));
                    }
                    let agent = crate::channels::telegram::TelegramAgent::new(
                        tg_agent_service,
                        self.channel_factory.service_context(),
                        self.channel_factory.shared_session_id(),
                        self.telegram_state.clone(),
                        self.channel_factory.config_rx(),
                        crate::db::ChannelMessageRepository::new(self.db_pool.clone()),
                    );
                    tracing::info!(
                        "ChannelManager: spawning Telegram bot ({} allowed users)",
                        tg.allowed_users.len()
                    );
                    // Overwrites any stale finished handle.
                    handles.insert("telegram".to_string(), agent.start(token.clone()));
                }
            }
            ChannelAction::Stop => {
                if let Some(handle) = handles.remove("telegram") {
                    tracing::info!("ChannelManager: stopping Telegram bot");
                    handle.abort();
                }
            }
            ChannelAction::Noop => {}
        }
    }

    /// Telegram userbot — the MTProto read plane. Runs only when the bot can
    /// (replies exit as the bot), a session file exists (login is a CLI
    /// action, not an agent task), and `channels.telegram.userbot.enabled`.
    /// Keyed separately from "telegram" so either plane restarts on its own.
    #[cfg(feature = "telegram-userbot")]
    async fn reconcile_telegram_userbot(
        &self,
        config: &Config,
        handles: &mut HashMap<String, JoinHandle<()>>,
    ) {
        let tg = &config.channels.telegram;
        let has_valid_token = tg
            .token
            .as_ref()
            .is_some_and(|t| !t.is_empty() && t.contains(':'));
        let should_run = tg.enabled && tg.userbot.enabled && has_valid_token;
        match channel_action(should_run, handle_alive(handles, "telegram-userbot")) {
            ChannelAction::Start => {
                let ub = &tg.userbot;
                if !crate::channels::telegram::userbot::login::session_exists(ub) {
                    tracing::info!(
                        "ChannelManager: Telegram userbot enabled but no session file — \
                         run `opencrabs channel userbot-login` to log in"
                    );
                    return;
                }
                let Some(token) = tg.token.clone() else {
                    return;
                };
                let bot = teloxide::Bot::new(token.clone());
                // Same resume/reaction wiring as the bot plane: the userbot's
                // AgentService gets the reaction queue and background-task
                // resume producer, with the weak-holder dance for the latter.
                let reaction_cb = self.telegram_state.reaction_queue_callback();
                let agent_holder: std::sync::Arc<
                    std::sync::Mutex<Option<std::sync::Weak<crate::brain::agent::AgentService>>>,
                > = std::sync::Arc::new(std::sync::Mutex::new(None));
                let enqueue_cb = crate::channels::telegram::resume::build_enqueue_callback(
                    self.telegram_state.clone(),
                    agent_holder.clone(),
                );
                let agent_service = self
                    .channel_factory
                    .create_agent_service_full(Some(reaction_cb), Some(enqueue_cb))
                    .await;
                if let Ok(mut h) = agent_holder.lock() {
                    *h = Some(std::sync::Arc::downgrade(&agent_service));
                }
                let deps = crate::channels::telegram::userbot::watch::UserbotDeps {
                    bot,
                    agent: agent_service,
                    session_svc: crate::services::SessionService::new(
                        self.channel_factory.service_context(),
                    ),
                    bot_token: std::sync::Arc::new(token),
                    shared_session: self.channel_factory.shared_session_id(),
                    telegram_state: self.telegram_state.clone(),
                    config_rx: self.channel_factory.config_rx(),
                    channel_msg_repo: crate::db::ChannelMessageRepository::new(
                        self.db_pool.clone(),
                    ),
                };
                match crate::channels::telegram::userbot::watch::spawn(deps).await {
                    Ok(handle) => {
                        tracing::info!(
                            "ChannelManager: spawning Telegram userbot (read plane, \
                             forwards `read`-granted chats as inbound)"
                        );
                        handles.insert("telegram-userbot".to_string(), handle);
                    }
                    Err(e) => {
                        tracing::error!("ChannelManager: Telegram userbot failed to start: {e}")
                    }
                }
            }
            ChannelAction::Stop => {
                if let Some(handle) = handles.remove("telegram-userbot") {
                    tracing::info!("ChannelManager: stopping Telegram userbot");
                    handle.abort();
                }
            }
            ChannelAction::Noop => {}
        }
    }

    #[cfg(feature = "whatsapp")]
    async fn reconcile_whatsapp(
        &self,
        config: &Config,
        handles: &mut HashMap<String, JoinHandle<()>>,
    ) {
        let wa = &config.channels.whatsapp;
        let should_run = wa.enabled;
        // A pairing reset wipes session.db and asks for a restart. Abort the
        // live agent so it starts fresh against the wiped session (drops old
        // auth at runtime); the Start arm below then respawns it.
        if should_run
            && self.whatsapp_state.take_restart_request()
            && let Some(handle) = handles.remove("whatsapp")
        {
            tracing::info!("ChannelManager: restarting WhatsApp agent for re-pairing");
            // Cleanly disconnect the live client BEFORE aborting the task.
            // Aborting the JoinHandle only drops the `bot.run()` future;
            // whatsapp-rust runs its keepalive and read loop on independent
            // detached tasks, so the old socket keeps pinging and lingers as a
            // second companion alongside the freshly-paired one. Two companions
            // on one session make WhatsApp drop a socket, so inbound messages
            // land on an orphaned connection and never get a reply.
            // `disconnect()` tears the transport down and disables
            // auto-reconnect so it cannot resurrect.
            if let Some(client) = self.whatsapp_state.client().await {
                client.disconnect().await;
            }
            handle.abort();
        }
        match channel_action(should_run, handle_alive(handles, "whatsapp")) {
            ChannelAction::Start => {
                // Background-task resume (#734): a finished detached command
                // resumes this session and replies to its WhatsApp chat.
                let agent_holder = crate::channels::bg_resume::new_holder();
                let enqueue_cb = crate::channels::whatsapp::resume::build_enqueue_callback(
                    self.whatsapp_state.clone(),
                    agent_holder.clone(),
                );
                let wa_agent_service = self
                    .channel_factory
                    .create_agent_service_full(None, Some(enqueue_cb))
                    .await;
                crate::channels::bg_resume::fill(&agent_holder, &wa_agent_service);
                let agent = crate::channels::whatsapp::WhatsAppAgent::new(
                    wa_agent_service,
                    self.channel_factory.service_context(),
                    self.channel_factory.shared_session_id(),
                    self.whatsapp_state.clone(),
                    self.channel_factory.config_rx(),
                    crate::db::ChannelMessageRepository::new(self.db_pool.clone()),
                );
                tracing::info!(
                    "ChannelManager: spawning WhatsApp agent ({} allowed phones)",
                    wa.allowed_phones.len()
                );
                // Overwrites any stale finished handle (dead agent auto-restarts).
                handles.insert("whatsapp".to_string(), agent.start());
            }
            ChannelAction::Stop => {
                if let Some(handle) = handles.remove("whatsapp") {
                    tracing::info!("ChannelManager: stopping WhatsApp agent");
                    // Same as the restart path: disconnect the live client so
                    // the socket does not linger after the task is aborted.
                    if let Some(client) = self.whatsapp_state.client().await {
                        client.disconnect().await;
                    }
                    handle.abort();
                }
            }
            ChannelAction::Noop => {}
        }
    }

    #[cfg(feature = "discord")]
    async fn reconcile_discord(
        &self,
        config: &Config,
        handles: &mut HashMap<String, JoinHandle<()>>,
    ) {
        let dc = &config.channels.discord;
        let has_valid_token = dc
            .token
            .as_ref()
            .map(|t| !t.is_empty() && t.len() > 50)
            .unwrap_or(false);
        let should_run = dc.enabled && has_valid_token;
        match channel_action(should_run, handle_alive(handles, "discord")) {
            ChannelAction::Start => {
                if let Some(ref token) = dc.token {
                    let token_hash = crate::config::profile::hash_token(token);
                    if let Err(e) =
                        crate::config::profile::acquire_token_lock("discord", &token_hash)
                    {
                        tracing::error!(
                            "ChannelManager: Discord token lock denied, this channel will NOT run: {}. \
                             Another profile is using the same bot credential; give each profile its own.",
                            e
                        );
                        return;
                    }
                    // Background-task resume (#732): a finished detached command
                    // resumes this session and replies to its Discord channel.
                    let agent_holder = crate::channels::bg_resume::new_holder();
                    let enqueue_cb = crate::channels::discord::resume::build_enqueue_callback(
                        self.discord_state.clone(),
                        agent_holder.clone(),
                    );
                    let dc_agent_service = self
                        .channel_factory
                        .create_agent_service_full(None, Some(enqueue_cb))
                        .await;
                    crate::channels::bg_resume::fill(&agent_holder, &dc_agent_service);
                    let agent = crate::channels::discord::DiscordAgent::new(
                        dc_agent_service,
                        self.channel_factory.service_context(),
                        self.channel_factory.shared_session_id(),
                        self.discord_state.clone(),
                        self.channel_factory.config_rx(),
                        crate::db::ChannelMessageRepository::new(self.db_pool.clone()),
                    );
                    tracing::info!(
                        "ChannelManager: spawning Discord bot ({} allowed users)",
                        dc.allowed_users.len()
                    );
                    handles.insert("discord".to_string(), agent.start(token.clone()));
                }
            }
            ChannelAction::Stop => {
                if let Some(handle) = handles.remove("discord") {
                    tracing::info!("ChannelManager: stopping Discord bot");
                    handle.abort();
                }
            }
            ChannelAction::Noop => {}
        }
    }

    #[cfg(feature = "slack")]
    async fn reconcile_slack(
        &self,
        config: &Config,
        handles: &mut HashMap<String, JoinHandle<()>>,
    ) {
        let sl = &config.channels.slack;
        let has_valid_tokens = sl
            .token
            .as_ref()
            .map(|t| !t.is_empty() && t.starts_with("xoxb-"))
            .unwrap_or(false)
            && sl
                .app_token
                .as_ref()
                .map(|t| !t.is_empty() && t.starts_with("xapp-"))
                .unwrap_or(false);
        let should_run = sl.enabled && has_valid_tokens;
        match channel_action(should_run, handle_alive(handles, "slack")) {
            ChannelAction::Start => {
                if let (Some(bot_tok), Some(app_tok)) = (sl.token.clone(), sl.app_token.clone()) {
                    let token_hash = crate::config::profile::hash_token(&bot_tok);
                    if let Err(e) = crate::config::profile::acquire_token_lock("slack", &token_hash)
                    {
                        tracing::error!(
                            "ChannelManager: Slack token lock denied, this channel will NOT run: {}. \
                             Another profile is using the same bot credential; give each profile its own.",
                            e
                        );
                        return;
                    }
                    // Background-task resume (#733): a finished detached command
                    // resumes this session and posts to its Slack channel.
                    let agent_holder = crate::channels::bg_resume::new_holder();
                    let enqueue_cb = crate::channels::slack::resume::build_enqueue_callback(
                        self.slack_state.clone(),
                        agent_holder.clone(),
                    );
                    let sl_agent_service = self
                        .channel_factory
                        .create_agent_service_full(None, Some(enqueue_cb))
                        .await;
                    crate::channels::bg_resume::fill(&agent_holder, &sl_agent_service);
                    let agent = crate::channels::slack::SlackAgent::new(
                        sl_agent_service,
                        self.channel_factory.service_context(),
                        self.channel_factory.shared_session_id(),
                        self.slack_state.clone(),
                        self.channel_factory.config_rx(),
                        crate::db::ChannelMessageRepository::new(self.db_pool.clone()),
                    );
                    tracing::info!(
                        "ChannelManager: spawning Slack bot ({} allowed users)",
                        sl.allowed_users.len()
                    );
                    handles.insert("slack".to_string(), agent.start(bot_tok, app_tok));
                }
            }
            ChannelAction::Stop => {
                if let Some(handle) = handles.remove("slack") {
                    tracing::info!("ChannelManager: stopping Slack bot");
                    handle.abort();
                }
            }
            ChannelAction::Noop => {}
        }
    }

    #[cfg(feature = "trello")]
    async fn reconcile_trello(
        &self,
        config: &Config,
        handles: &mut HashMap<String, JoinHandle<()>>,
    ) {
        let tr = &config.channels.trello;
        let has_valid_creds = tr
            .app_token
            .as_ref()
            .map(|k| !k.is_empty())
            .unwrap_or(false)
            && tr.token.as_ref().map(|t| !t.is_empty()).unwrap_or(false);
        let has_boards = !tr.board_ids.is_empty();
        let should_run = tr.enabled && has_valid_creds && has_boards;
        match channel_action(should_run, handle_alive(handles, "trello")) {
            ChannelAction::Start => {
                if let (Some(api_key), Some(api_token)) = (tr.app_token.clone(), tr.token.clone()) {
                    let token_hash = crate::config::profile::hash_token(&api_token);
                    if let Err(e) =
                        crate::config::profile::acquire_token_lock("trello", &token_hash)
                    {
                        tracing::error!(
                            "ChannelManager: Trello token lock denied, this channel will NOT run: {}. \
                             Another profile is using the same bot credential; give each profile its own.",
                            e
                        );
                        return;
                    }
                    let agent = crate::channels::trello::TrelloAgent::new(
                        self.channel_factory.create_agent_service().await,
                        self.channel_factory.service_context(),
                        tr.allowed_users.clone(),
                        self.channel_factory.shared_session_id(),
                        self.trello_state.clone(),
                        tr.board_ids.clone(),
                        tr.poll_interval_secs,
                        tr.session_idle_hours,
                    );
                    tracing::info!(
                        "ChannelManager: spawning Trello agent ({} boards)",
                        tr.board_ids.len()
                    );
                    handles.insert("trello".to_string(), agent.start(api_key, api_token));
                }
            }
            ChannelAction::Stop => {
                if let Some(handle) = handles.remove("trello") {
                    tracing::info!("ChannelManager: stopping Trello agent");
                    handle.abort();
                }
            }
            ChannelAction::Noop => {}
        }
    }
}
