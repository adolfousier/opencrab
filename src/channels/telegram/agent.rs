//! Telegram Agent
//!
//! Agent struct and startup logic.

use super::TelegramState;
use super::handler::{handle_message, handle_reaction};
use crate::brain::agent::AgentService;
use crate::config::Config;
use crate::db::ChannelMessageRepository;
use crate::services::{ServiceContext, SessionService};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use teloxide::prelude::*;
use teloxide::types::MessageId;
use tokio::sync::Mutex;
use uuid::Uuid;

/// Telegram bot that forwards messages to the agent
pub struct TelegramAgent {
    agent_service: Arc<AgentService>,
    session_service: SessionService,
    /// Shared session ID from the TUI — owner user shares the terminal session
    shared_session_id: Arc<Mutex<Option<Uuid>>>,
    telegram_state: Arc<TelegramState>,
    config_rx: tokio::sync::watch::Receiver<Config>,
    channel_msg_repo: ChannelMessageRepository,
}

impl TelegramAgent {
    pub fn new(
        agent_service: Arc<AgentService>,
        service_context: ServiceContext,
        shared_session_id: Arc<Mutex<Option<Uuid>>>,
        telegram_state: Arc<TelegramState>,
        config_rx: tokio::sync::watch::Receiver<Config>,
        channel_msg_repo: ChannelMessageRepository,
    ) -> Self {
        Self {
            agent_service,
            session_service: SessionService::new(service_context),
            shared_session_id,
            telegram_state,
            config_rx,
            channel_msg_repo,
        }
    }

    /// Start the bot as a background task. Returns a JoinHandle.
    pub fn start(self, token: String) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            // Validate token format BEFORE creating Bot: "numbers:alphanumeric"
            // e.g., "123456789:ABCdefGHIjklMNOpqrsTUVwxyz"
            if token.is_empty() {
                tracing::debug!("Telegram bot token is empty, skipping bot start");
                return;
            }

            if !token.contains(':') {
                tracing::debug!("Telegram bot token missing ':' separator, skipping bot start");
                return;
            }

            let parts: Vec<&str> = token.splitn(2, ':').collect();
            if parts.len() != 2 {
                tracing::debug!("Telegram bot token has invalid format, skipping bot start");
                return;
            }

            // First part must be numeric (bot ID)
            if parts[0].parse::<u64>().is_err() {
                tracing::debug!("Telegram bot token has invalid bot ID, skipping bot start");
                return;
            }

            // Second part must be at least 30 chars (API key)
            if parts[1].len() < 30 {
                tracing::debug!("Telegram bot token has too short API key, skipping bot start");
                return;
            }

            // Read initial config for logging
            let cfg = self.config_rx.borrow().clone();
            tracing::info!(
                "Starting Telegram bot with {} allowed user(s), STT={}, TTS={}",
                cfg.channels.telegram.allowed_users.len(),
                cfg.voice_config().stt_enabled,
                cfg.voice_config().tts_enabled,
            );

            let bot = Bot::new(token.clone());
            // Kept for the raw-aware update listener (`token` itself is moved
            // into the shared deps as `bot_token` further down).
            let listener_token = token.clone();

            // Verify token works with Telegram API before setting up dispatcher
            match bot.get_me().await {
                Ok(me) => {
                    if let Some(ref username) = me.username {
                        tracing::info!("Telegram: bot username is @{}", username);
                        self.telegram_state.set_bot_username(username.clone()).await;
                    }
                    // Store bot's numeric user ID for reply-to-bot detection
                    self.telegram_state
                        .set_bot_user_id(me.user.id.0 as i64)
                        .await;
                    tracing::info!("Telegram: bot user ID is {}", me.user.id.0);
                    // Store bot in state for proactive messaging only after successful auth
                    self.telegram_state.set_bot(bot.clone()).await;

                    // Register slash commands so they appear in Telegram's / menu
                    register_bot_commands(&bot).await;

                    // One-time: organize any pre-subdir flat attachments under
                    // channel_attachments/telegram/ (#513). Idempotent.
                    super::media::migrate_flat_channel_attachments();
                }
                Err(e) => {
                    tracing::warn!("Telegram: token validation failed: {}. Bot not started.", e);
                    return;
                }
            }

            let agent = self.agent_service.clone();
            let session_svc = self.session_service.clone();
            let bot_token = Arc::new(token);
            let shared_session = self.shared_session_id.clone();
            let telegram_state = self.telegram_state.clone();
            let config_rx = self.config_rx.clone();
            let channel_msg_repo = self.channel_msg_repo.clone();

            // Shared dependencies handed to every agent dispatch (first-frame,
            // settled-after-streaming, or immediate). `bot_token` and
            // `channel_msg_repo` are only needed here, so move them in; the
            // rest are cloned because the callback handler below reuses them.
            let deps = DispatchDeps {
                agent: agent.clone(),
                session_svc: session_svc.clone(),
                bot_token,
                shared_session: shared_session.clone(),
                telegram_state: telegram_state.clone(),
                config_rx: config_rx.clone(),
                channel_msg_repo,
            };

            // Pending edit-stream buffer keyed by (chat, message). Peer bots in
            // a group stream their replies by editing one message progressively
            // (the same mechanism we use). Reacting to the first frame makes us
            // read a half-written message and wrongly conclude it was "cut off".
            // We hold a bot's text message until its edit stream settles, then
            // process the FINAL text. See `spawn_settle_watcher`.
            let pending_edits: PendingEdits = Arc::new(Mutex::new(HashMap::new()));

            // ── Message handler ───────────────────────────────────────────────
            let msg_handler = Update::filter_message().endpoint({
                let deps = deps.clone();
                let pending_edits = pending_edits.clone();
                move |bot: Bot, msg: Message| {
                    let deps = deps.clone();
                    let pending_edits = pending_edits.clone();
                    async move {
                        // A peer bot's first frame: defer until the stream
                        // settles instead of processing the partial.
                        if is_stream_candidate(&msg) {
                            let key = (msg.chat.id, msg.id);
                            let generation = {
                                let mut map = pending_edits.lock().await;
                                let entry = map.entry(key).or_insert((0, msg.clone()));
                                entry.0 += 1;
                                entry.1 = msg.clone();
                                entry.0
                            };
                            spawn_settle_watcher(key, generation, pending_edits, bot, deps);
                            return ResponseResult::Ok(());
                        }
                        // Everything else (humans, DMs, non-text) processes
                        // immediately, in the background so the dispatcher stays
                        // free for callback queries while the agent runs.
                        spawn_handle_message(bot, msg, deps);
                        ResponseResult::Ok(())
                    }
                }
            });

            // ── Edited-message handler ────────────────────────────────────────
            // Receives the progressive edits of a peer bot's streamed reply.
            // While the message is still pending we reset its settle timer to
            // the newest frame; once it has already been processed we only
            // reconcile stored history so context reflects the final text.
            let edited_handler = Update::filter_edited_message().endpoint({
                let deps = deps.clone();
                let pending_edits = pending_edits.clone();
                move |bot: Bot, msg: Message| {
                    let deps = deps.clone();
                    let pending_edits = pending_edits.clone();
                    async move {
                        if !is_stream_candidate(&msg) {
                            return ResponseResult::Ok(());
                        }
                        let key = (msg.chat.id, msg.id);
                        let generation = {
                            let mut map = pending_edits.lock().await;
                            map.get_mut(&key).map(|entry| {
                                entry.0 += 1;
                                entry.1 = msg.clone();
                                entry.0
                            })
                        };
                        match generation {
                            // Still streaming — push the settle deadline out.
                            Some(generation) => {
                                spawn_settle_watcher(key, generation, pending_edits, bot, deps);
                            }
                            // Edit landed after we processed the settled message
                            // (e.g. a manual late edit). Rewrite stored history;
                            // don't reprocess — that would loop on every edit.
                            None => {
                                if let Some(text) = msg.text() {
                                    let chat_id = msg.chat.id.0.to_string();
                                    let pmid = msg.id.0.to_string();
                                    if let Err(e) = deps
                                        .channel_msg_repo
                                        .update_content("telegram", &chat_id, &pmid, text)
                                        .await
                                    {
                                        tracing::warn!(
                                            "Telegram: failed to reconcile edited message \
                                             {pmid} content: {e}"
                                        );
                                    }
                                }
                            }
                        }
                        ResponseResult::Ok(())
                    }
                }
            });

            // ── Callback query handler (for Approve / Deny inline buttons) ────
            let cb_handler = Update::filter_callback_query().endpoint({
                let telegram_state = telegram_state.clone();
                let agent = agent.clone();
                let session_svc = session_svc.clone();
                let shared_session = shared_session.clone();
                let config_rx = config_rx.clone();
                move |bot: Bot, query: CallbackQuery| {
                    let state = telegram_state.clone();
                    let agent = agent.clone();
                    let session_svc = session_svc.clone();
                    let shared_session = shared_session.clone();
                    let config_rx = config_rx.clone();
                    async move {
                        if let Some(data) = query.data.as_deref() {
                            tracing::info!("Telegram callback query received: data={}", data);

                            // Setup callback for unconfigured providers — show the
                            // help text from `unconfigured_provider_help` instead
                            // of trying to switch (no API key would just fail).
                            // Bots cannot delete user messages in DMs, so we
                            // never prompt for the key inline (issue #126, B.1).
                            if let Some(provider_name) = data.strip_prefix("setup:") {
                                let help = crate::channels::commands::unconfigured_provider_help(provider_name);
                                if let Some(msg_ref) = query.message.as_ref() {
                                    let chat_id = msg_ref.chat().id;
                                    let thread_id = msg_ref.regular_message().and_then(|m| m.thread_id);
                                    let _ = crate::channels::telegram::send::message_in_thread(
                                        &bot,
                                        chat_id,
                                        thread_id,
                                        crate::channels::telegram::handler::md_to_html(&help),
                                    )
                                    .parse_mode(teloxide::types::ParseMode::Html)
                                    .await;
                                }
                                let _ = bot.answer_callback_query(query.id.clone()).await;
                                return Ok::<(), teloxide::RequestError>(());
                            }

                            // Provider picker callback → show models for that provider
                            if let Some(provider_name) = data.strip_prefix("provider:") {
                                let resp = crate::channels::commands::models_for_provider(provider_name).await;

                                // Agent-handled providers (OpenRouter 300+ models, custom)
                                // Switch to default if set, then let the agent follow up.
                                if resp.agent_handled {
                                    // Resolve session from the chat where the button was pressed,
                                    // not from shared_session (which is the TUI session).
                                    let session_id = resolve_callback_session(&query, &state, &shared_session).await;
                                    let display = crate::channels::commands::provider_display_name(provider_name);
                                    // Switch to this provider with its default model. Pin the
                                    // provider to THIS session so another channel/session
                                    // doesn't get yanked onto it — the model callback's
                                    // switch_model then reads from the same per-session slot.
                                    let config = crate::config::Config::current();
                                    if let Ok(new_provider) = crate::brain::provider::factory::create_provider_by_name(&config, provider_name).await
                                    {
                                        match session_id {
                                            Some(sid) => agent.swap_provider_for_session(sid, new_provider.clone(), new_provider.default_model().to_string()),
                                            None => agent.swap_provider(new_provider),
                                        }
                                    }
                                    if !resp.current_model.is_empty() {
                                        let _ = crate::channels::commands::switch_model(&agent, &resp.current_model, session_id, Some(provider_name)).await;
                                    }
                                    let _ = bot.answer_callback_query(query.id.clone()).await;
                                    // Send synthetic message to agent so it handles follow-up
                                    let prompt = if resp.current_model.is_empty() {
                                        format!(
                                            "[System: User selected {} provider but no default model is set. \
                                             Ask them which model they want. Use config_manager tool to read \
                                             providers section, then set the default_model. Keep current provider \
                                             until a model is chosen.]",
                                            display
                                        )
                                    } else {
                                        format!(
                                            "[System: User switched to {} provider with model {}. \
                                             Confirm the switch. Ask if they want a different model — \
                                             if so, use config_manager to update providers.{}.default_model \
                                             and confirm.]",
                                            display, resp.current_model,
                                            if provider_name == "openrouter" { "openrouter" } else { provider_name }
                                        )
                                    };
                                    if let Some(sid) = session_id {
                                        let agent_clone = agent.clone();
                                        let bot_clone = bot.clone();
                                        let (chat_id, thread_id) = query
                                            .message
                                            .as_ref()
                                            .map(|m| {
                                                (
                                                    m.chat().id,
                                                    m.regular_message().and_then(|r| r.thread_id),
                                                )
                                            })
                                            .unwrap_or((teloxide::types::ChatId(0), None));
                                        tokio::spawn(async move {
                                            match agent_clone.send_message(sid, prompt, None).await {
                                                Ok(resp) => {
                                                    let clean = crate::utils::sanitize::strip_llm_artifacts(&resp.content);
                                                    let html = crate::channels::telegram::handler::md_to_html(&clean);
                                                    let _ = crate::channels::telegram::send::message_in_thread(
                                                        &bot_clone, chat_id, thread_id, html,
                                                    )
                                                    .parse_mode(teloxide::types::ParseMode::Html)
                                                    .await;
                                                }
                                                Err(e) => {
                                                    tracing::error!("Agent follow-up failed: {}", e);
                                                }
                                            }
                                        });
                                    }
                                    return ResponseResult::Ok(());
                                }

                                if resp.models.is_empty() {
                                    let _ = bot
                                        .answer_callback_query(query.id.clone())
                                        .text("No models available for this provider")
                                        .await;
                                    return ResponseResult::Ok(());
                                }
                                let _ = bot.answer_callback_query(query.id.clone()).await;
                                if let Some(msg) = &query.message {
                                    use teloxide::payloads::EditMessageTextSetters;
                                    use teloxide::prelude::Requester;
                                    use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};
                                    let rows: Vec<Vec<InlineKeyboardButton>> = resp
                                        .models
                                        .iter()
                                        .enumerate()
                                        .map(|(i, m)| {
                                            let display = if *m == resp.current_model {
                                                format!("✓ {}", m)
                                            } else {
                                                m.clone()
                                            };
                                            // Pipe separator because BOTH provider_name and
                                            // model can contain `:` — custom providers
                                            // are `custom:<name>` (e.g. `custom:dialagram`)
                                            // and OpenRouter models carry `:free`/`:thinking`
                                            // suffixes. Splitting on `:` here put the
                                            // provider's tail into the model name and the
                                            // session got persisted with broken metadata
                                            // (`provider=custom`, `model=dialagram:qwen-3.7-…`
                                            // — seen 2026-05-18T23:39 sync_provider trace).
                                            // Generator + parser MUST stay in lock-step.
                                            // Telegram caps callback_data at 64 BYTES. Long model
                                            // names (e.g. modelscope's
                                            // "deepseek-ai/DeepSeek-R1-Distill-Qwen-1.5B" → 65 B)
                                            // overflow, and Telegram then rejects the ENTIRE
                                            // keyboard (BUTTON_DATA_INVALID) — the picker showed
                                            // nothing and spun on "loading" forever. This helper
                                            // falls back to an index form the parser resolves.
                                            let data = crate::channels::commands::model_button_callback_data(
                                                &resp.provider_name, m, i,
                                            );
                                            vec![InlineKeyboardButton::callback(display, data)]
                                        })
                                        .collect();
                                    let keyboard = InlineKeyboardMarkup::new(rows);
                                    let text = crate::channels::telegram::handler::md_to_html(&resp.text);
                                    let _ = bot
                                        .edit_message_text(msg.chat().id, msg.id(), &text)
                                        .parse_mode(teloxide::types::ParseMode::Html)
                                        .reply_markup(keyboard)
                                        .await;
                                }
                                return ResponseResult::Ok(());
                            }

                            // Model switch callback (format: model:<provider>|<model>).
                            // Pipe — not colon — between provider and model so a
                            // custom-provider name (`custom:dialagram`) and an
                            // OpenRouter-style model suffix (`:free`, `:thinking`)
                            // don't fold into each other on parse.
                            // Apply-to-all-sessions (#468): bulk-write the pair
                            // the user just switched to. The current session is
                            // already switched; this covers the rest, which pick
                            // it up on their next message.
                            if let Some(rest) = data.strip_prefix("allm:") {
                                let reply = if let Some((prov, model)) = rest.split_once('|') {
                                    let session_svc = crate::services::SessionService::new(
                                        agent.context().clone(),
                                    );
                                    match session_svc
                                        .set_provider_model_all_sessions(prov, model)
                                        .await
                                    {
                                        Ok(n) => format!(
                                            "✅ {prov}/{model} applied to {n} other session(s); \
                                             each picks it up on its next message."
                                        ),
                                        Err(e) => format!("⚠️ Scope-all write failed: {e}"),
                                    }
                                } else {
                                    "⚠️ Malformed apply-all payload.".to_string()
                                };
                                let _ = bot.answer_callback_query(query.id.clone()).await;
                                if let Some(msg) = &query.message {
                                    use teloxide::prelude::Requester;
                                    let _ = bot.send_message(msg.chat().id, reply).await;
                                }
                                return ResponseResult::Ok(());
                            }
                            if let Some(rest) = data.strip_prefix("model:") {
                                let (provider_name, model_token) = if let Some((p, m)) = rest.split_once('|') {
                                    (Some(p), m)
                                } else {
                                    (None, rest)
                                };
                                // An index-encoded button (model:<provider>|#<i>) — the
                                // generator falls back to this when the literal model name
                                // would overflow Telegram's 64-byte callback_data. Resolve
                                // it back to the name via the same config list. Plain names
                                // (short models, other callers) pass through unchanged.
                                let resolved_owned: Option<String> =
                                    match (provider_name, model_token.strip_prefix('#')) {
                                        (Some(pname), Some(idx)) => idx
                                            .parse::<usize>()
                                            .ok()
                                            .and_then(|i| {
                                                crate::channels::commands::model_at_index(pname, i)
                                            }),
                                        _ => None,
                                    };
                                let model_name: &str =
                                    resolved_owned.as_deref().unwrap_or(model_token);
                                // Resolve session BEFORE the provider swap so the swap
                                // lands on the right per-session slot. Leaving the
                                // resolve below the swap (as it was) made the provider
                                // change visible to other sessions via the global slot
                                // until the per-session pin from switch_model landed.
                                let session_id = resolve_callback_session(&query, &state, &shared_session).await;
                                // Switch provider if specified and different
                                let mut provider_err: Option<String> = None;
                                if let Some(pname) = provider_name {
                                    match crate::config::Config::load() {
                                        Ok(config) => match crate::brain::provider::factory::create_provider_by_name(&config, pname).await {
                                            Ok(new_provider) => match session_id {
                                                Some(sid) => agent.swap_provider_for_session(sid, new_provider.clone(), new_provider.default_model().to_string()),
                                                None => agent.swap_provider(new_provider),
                                            },
                                            Err(e) => provider_err = Some(format!("Failed to create provider '{}': {}", pname, e)),
                                        },
                                        Err(e) => provider_err = Some(format!("Failed to load config: {}", e)),
                                    }
                                }
                                let (switch_ok, display_text) = if let Some(err) = provider_err {
                                    (false, format!("⚠️ {}", err))
                                } else {
                                    match crate::channels::commands::switch_model(&agent, model_name, session_id, provider_name).await {
                                        Ok(_) => (true, format!("✅ Model switched to <code>{}</code>", model_name)),
                                        Err(e) => (false, format!("⚠️ {}", e)),
                                    }
                                };
                                let _ = bot.answer_callback_query(query.id.clone()).await;
                                if let Some(msg) = &query.message {
                                    use teloxide::payloads::EditMessageTextSetters;
                                    use teloxide::prelude::Requester;
                                    let _ = bot
                                        .edit_message_text(msg.chat().id, msg.id(), &display_text)
                                        .parse_mode(teloxide::types::ParseMode::Html)
                                        .reply_markup(
                                            teloxide::types::InlineKeyboardMarkup::default(),
                                        )
                                        .await;
                                }
                                if !switch_ok {
                                    tracing::warn!("Telegram model switch failed: {}", display_text);
                                }
                                return ResponseResult::Ok(());
                            }

                            // Session switch callback
                            if let Some(session_id_str) = data.strip_prefix("session:") {
                                if let Ok(new_id) = session_id_str.parse::<Uuid>() {
                                    // Determine if caller is owner
                                    let cfg = config_rx.borrow().clone();
                                    let caller_id_raw = query.from.id.0;
                                    let is_owner = cfg.channels.telegram.is_owner(&caller_id_raw.to_string());

                                    if is_owner {
                                        *shared_session.lock().await = Some(new_id);
                                    }
                                    // Owner and guest: bind chat_id → session_id for
                                    // handle_message (issue #121). Guest extra_sessions
                                    // map was removed — it was never read on ingest.
                                    let switch_chat_id = query
                                        .message
                                        .as_ref()
                                        .map(|m| m.chat().id.0)
                                        .unwrap_or(caller_id_raw as i64);
                                    // Scope the binding to the forum topic the switch
                                    // happened in, so the next message in that topic
                                    // resolves to the switched session (#215).
                                    let switch_topic_id =
                                        query.message.as_ref().and_then(|m| m.regular_message()).and_then(|m| {
                                            super::session_resolve::topic_session_id(
                                                m.is_topic_message,
                                                m.thread_id.map(|t| t.0.0),
                                            )
                                        });
                                    state
                                        .register_session_chat(new_id, switch_chat_id, switch_topic_id)
                                        .await;

                                    // Touch updated_at so find_session_by_title_suffix returns this session on next message
                                    if let Ok(Some(s)) = session_svc.get_session(new_id).await {
                                        let _ = session_svc.update_session(&s).await;
                                    }

                                    let _ = bot
                                        .answer_callback_query(query.id.clone())
                                        .text("Session switched")
                                        .await;
                                    if let Some(msg) = &query.message {
                                        use teloxide::payloads::EditMessageTextSetters;
                                        use teloxide::prelude::Requester;
                                        let _ = bot
                                            .edit_message_text(
                                                msg.chat().id,
                                                msg.id(),
                                                {
                                                    let display = match session_svc.get_session(new_id).await {
                                                        Ok(Some(s)) => s.title.unwrap_or_else(|| session_id_str[..8.min(session_id_str.len())].to_string()),
                                                        _ => session_id_str[..8.min(session_id_str.len())].to_string(),
                                                    };
                                                    format!("✅ Switched to session <code>{}</code>", display)
                                                },
                                            )
                                            .parse_mode(teloxide::types::ParseMode::Html)
                                            .reply_markup(
                                                teloxide::types::InlineKeyboardMarkup::default(),
                                            )
                                            .await;
                                    }
                                } else {
                                    let _ = bot
                                        .answer_callback_query(query.id.clone())
                                        .text("Invalid session ID")
                                        .await;
                                }
                                return ResponseResult::Ok(());
                            }

                            // Follow-up question callback: `q:<id>:<idx>`.
                            // Handled separately from the approve/deny chain
                            // because it returns an option string, not a
                            // boolean.
                            if let Some(rest) = data.strip_prefix("q:") {
                                let mut parts = rest.splitn(2, ':');
                                let q_id = parts.next().unwrap_or("");
                                let idx_str = parts.next().unwrap_or("");
                                let idx: usize = idx_str.parse().unwrap_or(usize::MAX);
                                let resolved = state
                                    .resolve_pending_question(q_id, idx)
                                    .await;
                                tracing::info!(
                                    "Telegram follow_up_question resolved: id={} idx={} answer={:?}",
                                    q_id,
                                    idx,
                                    resolved
                                );
                                let _ = bot.answer_callback_query(query.id.clone()).await;
                                if let Some(answer) = resolved
                                    && let Some(msg) = &query.message
                                {
                                    let original_text = match msg {
                                        teloxide::types::MaybeInaccessibleMessage::Regular(m) => {
                                            m.text().unwrap_or("").to_string()
                                        }
                                        _ => String::new(),
                                    };
                                    let updated =
                                        format!("{}\n\n✅ {}", original_text, answer);
                                    use teloxide::payloads::EditMessageTextSetters;
                                    use teloxide::prelude::Requester;
                                    if let Err(e) = bot
                                        .edit_message_text(msg.chat().id, msg.id(), &updated)
                                        .reply_markup(
                                            teloxide::types::InlineKeyboardMarkup::default(),
                                        )
                                        .await
                                    {
                                        tracing::error!(
                                            "Telegram: failed to edit question message: {}",
                                            e
                                        );
                                    }
                                }
                                return ResponseResult::Ok(());
                            }

                            // Directory browser callbacks: cd:sel:{idx}, cd:up, cd:pg:{n}, cd:here, cd:noop
                            if data.starts_with("cd:") {
                                // Owner-only: the browser exposes the host
                                // filesystem. Even though /cd is owner-gated, the
                                // inline keyboard sits in the chat where any
                                // allowlisted user could tap it, so re-check the
                                // tapper here.
                                let caller_is_owner = config_rx
                                    .borrow()
                                    .channels
                                    .telegram
                                    .is_owner(&query.from.id.0.to_string());
                                if !caller_is_owner {
                                    let _ = bot
                                        .answer_callback_query(query.id.clone())
                                        .text("🔒 Owner only")
                                        .show_alert(true)
                                        .await;
                                    return ResponseResult::Ok(());
                                }
                                let chat_id = query.message.as_ref().map(|m| m.chat().id.0).unwrap_or(0);
                                // Answer callback query — deferred to each branch
                                // to allow custom text popups (cd:sel on files).
                                let topic_id = query.message.as_ref()
                                    .and_then(|m| m.regular_message())
                                    .and_then(|r| r.thread_id)
                                    .map(|t| t.0.0);

                                // Handle cd:noop (page indicator, no action)
                                if data == "cd:noop" {
                                    let _ = bot.answer_callback_query(query.id.clone()).await;
                                    return ResponseResult::Ok(());
                                }

                                // Get current browser state
                                let browser_state = state.get_dir_browser(chat_id, topic_id).await;
                                let (current_path, current_filter) = browser_state.unwrap_or_else(|| {
                                    let home = dirs::home_dir()
                                        .map(|p| p.to_string_lossy().to_string())
                                        .unwrap_or_else(|| "/".to_string());
                                    (home, None)
                                });

                                let new_state: Option<crate::channels::commands::DirBrowserResponse> = if data == "cd:up" {
                                    let _ = bot.answer_callback_query(query.id.clone()).await;
                                    // Navigate to parent
                                    let parent = std::path::PathBuf::from(&current_path)
                                        .parent()
                                        .map(|p| p.to_string_lossy().to_string())
                                        .unwrap_or_else(|| "/".to_string());
                                    Some(crate::channels::commands::rebuild_cd_browser(&parent, 0, current_filter.as_deref()))
                                } else if data == "cd:here" {
                                    let _ = bot.answer_callback_query(query.id.clone()).await;
                                    // Confirm directory — set as working dir
                                    let session_id = resolve_callback_session(&query, &state, &shared_session).await;
                                    if let Some(sid) = session_id {
                                        // Update runtime working directory
                                        let wd = agent.shared_working_directory();
                                        *wd.write().expect("working_directory lock poisoned") = std::path::PathBuf::from(&current_path);
                                        // Persist to session DB
                                        let svc = crate::services::SessionService::new(agent.context().clone());
                                        let _ = svc.update_session_working_directory(sid, Some(current_path.clone())).await;
                                    }
                                    state.clear_dir_browser(chat_id, topic_id).await;
                                    // Edit the message to confirm
                                    if let Some(msg) = &query.message {
                                        use teloxide::payloads::EditMessageTextSetters;
                                        use teloxide::prelude::Requester;
                                        let confirm_text = format!("✅ Working directory set to:\n<code>{}</code>", current_path);
                                        if let Err(e) = bot
                                            .edit_message_text(msg.chat().id, msg.id(), &confirm_text)
                                            .parse_mode(teloxide::types::ParseMode::Html)
                                            .reply_markup(teloxide::types::InlineKeyboardMarkup::default())
                                            .await
                                        {
                                            tracing::warn!("cd:here: failed to edit message: {}", e);
                                        }
                                    }
                                    return ResponseResult::Ok(());
                                } else if let Some(idx_str) = data.strip_prefix("cd:sel:") {
                                    // Select entry by index from full listing
                                    let idx: usize = idx_str.parse().unwrap_or(0);
                                    let all_entries = crate::channels::commands::read_dir_entries(
                                        &std::path::PathBuf::from(&current_path),
                                        current_filter.as_deref(),
                                    ).0;
                                    if let Some(entry) = all_entries.get(idx) {
                                        if entry.is_dir {
                                            let _ = bot.answer_callback_query(query.id.clone()).await;
                                            // Navigate into directory
                                            let new_path = std::path::PathBuf::from(&current_path)
                                                .join(&entry.name)
                                                .to_string_lossy()
                                                .to_string();
                                            Some(crate::channels::commands::rebuild_cd_browser(&new_path, 0, current_filter.as_deref()))
                                        } else {
                                            // File selected — just show info, stay in same dir
                                            let file_path = std::path::PathBuf::from(&current_path)
                                                .join(&entry.name);
                                            // Answer with a popup showing the file path
                                            let _ = bot.answer_callback_query(query.id.clone())
                                                .text(format!("📄 {}", file_path.display()))
                                                .show_alert(true)
                                                .await;
                                            Some(crate::channels::commands::rebuild_cd_browser(&current_path, 0, current_filter.as_deref()))
                                        }
                                    } else {
                                        None
                                    }
                                } else if let Some(pg_str) = data.strip_prefix("cd:pg:") {
                                    let _ = bot.answer_callback_query(query.id.clone()).await;
                                    // Page navigation
                                    let page: usize = pg_str.parse().unwrap_or(0);
                                    Some(crate::channels::commands::rebuild_cd_browser(&current_path, page, current_filter.as_deref()))
                                } else {
                                    let _ = bot.answer_callback_query(query.id.clone()).await;
                                    None
                                };

                                if let Some(resp) = new_state {
                                    state.set_dir_browser(chat_id, topic_id, resp.current_path.clone(), resp.filter.clone()).await;
                                    let rows = crate::channels::telegram::handler::build_cd_keyboard(&resp);
                                    let keyboard = teloxide::types::InlineKeyboardMarkup::new(rows);
                                    if let Some(msg) = &query.message {
                                        use teloxide::payloads::EditMessageTextSetters;
                                        use teloxide::prelude::Requester;
                                        let html = crate::channels::telegram::handler::md_to_html(&resp.text);
                                        if let Err(e) = bot
                                            .edit_message_text(msg.chat().id, msg.id(), &html)
                                            .parse_mode(teloxide::types::ParseMode::Html)
                                            .reply_markup(keyboard)
                                            .await
                                        {
                                            tracing::warn!("cd:navigate: failed to edit message: {}", e);
                                        }
                                    }
                                }
                                return ResponseResult::Ok(());
                            }

                            // Profile manager callbacks: prof:sel:{name}, prof:create,
                            // prof:migrate:{name}, prof:del:{name}, prof:confirm_migrate:{name},
                            // prof:confirm_del:{name}, prof:back
                            if data.starts_with("prof:") {
                                use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};
                                let chat_id = query.message.as_ref().map(|m| m.chat().id.0).unwrap_or(0);
                                let _ = bot.answer_callback_query(query.id.clone()).await;

                                if let Some(name) = data.strip_prefix("prof:sel:") {
                                    // Show profile detail view with action buttons
                                    let profiles = crate::config::profile::list_profiles().unwrap_or_default();
                                    let active = crate::config::profile::active_profile().unwrap_or("default");
                                    if let Some(entry) = profiles.iter().find(|p| p.name == name) {
                                        let is_active = entry.name == active;
                                        let mut text = format!("👤 *Profile:* `{}`\n", entry.name);
                                        if let Some(desc) = &entry.description {
                                            text.push_str(&format!("📝 {}\n", desc));
                                        }
                                        text.push_str(&format!("📅 Created: {}\n", entry.created_at));
                                        if let Some(used) = &entry.last_used {
                                            text.push_str(&format!("⏰ Last used: {}\n", used));
                                        }
                                        if is_active {
                                            text.push_str("\n✅ *This is the active profile*");
                                        }

                                        let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
                                        if !is_active {
                                            rows.push(vec![InlineKeyboardButton::callback(
                                                "🔄 Migrate from current",
                                                format!("prof:migrate:{}", entry.name),
                                            )]);
                                        }
                                        if entry.name != "default" && !is_active {
                                            rows.push(vec![InlineKeyboardButton::callback(
                                                "🗑️ Delete",
                                                format!("prof:del:{}", entry.name),
                                            )]);
                                        }
                                        rows.push(vec![InlineKeyboardButton::callback(
                                            "◀️ Back to Profiles",
                                            "prof:back".to_string(),
                                        )]);

                                        let keyboard = InlineKeyboardMarkup::new(rows);
                                        if let Some(msg) = &query.message {
                                            use teloxide::payloads::EditMessageTextSetters;
                                            use teloxide::prelude::Requester;
                                            let html = crate::channels::telegram::handler::md_to_html(&text);
                                            if let Err(e) = bot
                                                .edit_message_text(msg.chat().id, msg.id(), &html)
                                                .parse_mode(teloxide::types::ParseMode::Html)
                                                .reply_markup(keyboard)
                                                .await
                                            {
                                                tracing::warn!("prof:sel: failed to edit message: {}", e);
                                            }
                                        }
                                    }
                                    return ResponseResult::Ok(());
                                }

                                if data == "prof:create" {
                                    // Prompt user for new profile name
                                    // Store the create state for this chat
                                    state.set_prof_create(chat_id, true).await;
                                    if let Some(msg) = &query.message {
                                        use teloxide::payloads::SendMessageSetters;
                                        use teloxide::prelude::Requester;
                                        let prompt = "✏️ Send me the name for the new profile:\n\n\
                                            (Letters, numbers, hyphens, underscores. 1-64 chars.)";
                                        let sent = bot
                                            .send_message(msg.chat().id, prompt)
                                            .reply_markup(InlineKeyboardMarkup::new(vec![vec![
                                                InlineKeyboardButton::callback("❌ Cancel", "prof:back"),
                                            ]]))
                                            .await;
                                        if let Err(e) = sent {
                                            tracing::warn!("prof:create: failed to send prompt: {}", e);
                                        }
                                    }
                                    return ResponseResult::Ok(());
                                }

                                if let Some(name) = data.strip_prefix("prof:migrate:") {
                                    // Show migration confirmation
                                    let active = crate::config::profile::active_profile().unwrap_or("default");
                                    let text = format!(
                                        "🔄 *Migrate Profile*\n\n\
                                        This will copy all brain files, config, keys, and memory from \
                                        `{}` → `{}`.\n\n\
                                        Existing files in `{}` will be overwritten (force=true).\n\n\
                                        Confirm?",
                                        active, name, name
                                    );
                                    let keyboard = InlineKeyboardMarkup::new(vec![
                                        vec![InlineKeyboardButton::callback(
                                            "✅ Confirm migration",
                                            format!("prof:confirm_migrate:{}", name),
                                        )],
                                        vec![InlineKeyboardButton::callback(
                                            "❌ Cancel",
                                            format!("prof:sel:{}", name),
                                        )],
                                    ]);
                                    if let Some(msg) = &query.message {
                                        use teloxide::payloads::EditMessageTextSetters;
                                        use teloxide::prelude::Requester;
                                        let html = crate::channels::telegram::handler::md_to_html(&text);
                                        if let Err(e) = bot
                                            .edit_message_text(msg.chat().id, msg.id(), &html)
                                            .parse_mode(teloxide::types::ParseMode::Html)
                                            .reply_markup(keyboard)
                                            .await
                                        {
                                            tracing::warn!("prof:migrate: failed to edit message: {}", e);
                                        }
                                    }
                                    return ResponseResult::Ok(());
                                }

                                if let Some(name) = data.strip_prefix("prof:confirm_migrate:") {
                                    let active = crate::config::profile::active_profile().unwrap_or("default");
                                    match crate::config::profile::migrate_profile(active, name, true) {
                                        Ok(files) => {
                                            let text = format!(
                                                "✅ Migration complete!\n\n\
                                                Copied {} files from `{}` → `{}`.\n\n\
                                                Restart with `-p {}` to switch profiles.",
                                                files.len(), active, name, name
                                            );
                                            if let Some(msg) = &query.message {
                                                use teloxide::payloads::EditMessageTextSetters;
                                                use teloxide::prelude::Requester;
                                                let html = crate::channels::telegram::handler::md_to_html(&text);
                                                let _ = bot
                                                    .edit_message_text(msg.chat().id, msg.id(), &html)
                                                    .parse_mode(teloxide::types::ParseMode::Html)
                                                    .reply_markup(InlineKeyboardMarkup::default())
                                                    .await;
                                            }
                                        }
                                        Err(e) => {
                                            let text = format!("❌ Migration failed: {}", e);
                                            if let Some(msg) = &query.message {
                                                use teloxide::payloads::EditMessageTextSetters;
                                                use teloxide::prelude::Requester;
                                                if let Err(e2) = bot
                                                    .edit_message_text(msg.chat().id, msg.id(), &text)
                                                    .reply_markup(InlineKeyboardMarkup::default())
                                                    .await
                                                {
                                                    tracing::warn!("prof:confirm_migrate: failed to edit: {}", e2);
                                                }
                                            }
                                        }
                                    }
                                    return ResponseResult::Ok(());
                                }

                                if let Some(name) = data.strip_prefix("prof:del:") {
                                    let text = format!(
                                        "🗑️ *Delete Profile*\n\n\
                                        Are you sure you want to delete `{}`?\n\
                                        This removes all files in the profile directory.\n\n\
                                        This cannot be undone.",
                                        name
                                    );
                                    let keyboard = InlineKeyboardMarkup::new(vec![
                                        vec![InlineKeyboardButton::callback(
                                            "✅ Confirm delete",
                                            format!("prof:confirm_del:{}", name),
                                        )],
                                        vec![InlineKeyboardButton::callback(
                                            "❌ Cancel",
                                            "prof:back".to_string(),
                                        )],
                                    ]);
                                    if let Some(msg) = &query.message {
                                        use teloxide::payloads::EditMessageTextSetters;
                                        use teloxide::prelude::Requester;
                                        let html = crate::channels::telegram::handler::md_to_html(&text);
                                        if let Err(e) = bot
                                            .edit_message_text(msg.chat().id, msg.id(), &html)
                                            .parse_mode(teloxide::types::ParseMode::Html)
                                            .reply_markup(keyboard)
                                            .await
                                        {
                                            tracing::warn!("prof:del: failed to edit message: {}", e);
                                        }
                                    }
                                    return ResponseResult::Ok(());
                                }

                                if let Some(name) = data.strip_prefix("prof:confirm_del:") {
                                    match crate::config::profile::delete_profile(name) {
                                        Ok(()) => {
                                            let text = format!("✅ Profile `{}` deleted.", name);
                                            if let Some(msg) = &query.message {
                                                use teloxide::payloads::EditMessageTextSetters;
                                                use teloxide::prelude::Requester;
                                                let html = crate::channels::telegram::handler::md_to_html(&text);
                                                let _ = bot
                                                    .edit_message_text(msg.chat().id, msg.id(), &html)
                                                    .parse_mode(teloxide::types::ParseMode::Html)
                                                    .reply_markup(InlineKeyboardMarkup::default())
                                                    .await;
                                            }
                                        }
                                        Err(e) => {
                                            let text = format!("❌ Delete failed: {}", e);
                                            if let Some(msg) = &query.message {
                                                use teloxide::payloads::EditMessageTextSetters;
                                                use teloxide::prelude::Requester;
                                                let _ = bot
                                                    .edit_message_text(msg.chat().id, msg.id(), &text)
                                                    .reply_markup(InlineKeyboardMarkup::default())
                                                    .await;
                                            }
                                        }
                                    }
                                    return ResponseResult::Ok(());
                                }

                                if data == "prof:back" {
                                    // Return to profiles list
                                    let resp = crate::channels::commands::format_profiles_browser().await;
                                    let rows = crate::channels::telegram::handler::build_profiles_keyboard(&resp);
                                    let keyboard = InlineKeyboardMarkup::new(rows);
                                    if let Some(msg) = &query.message {
                                        use teloxide::payloads::EditMessageTextSetters;
                                        use teloxide::prelude::Requester;
                                        let html = crate::channels::telegram::handler::md_to_html(&resp.text);
                                        if let Err(e) = bot
                                            .edit_message_text(msg.chat().id, msg.id(), &html)
                                            .parse_mode(teloxide::types::ParseMode::Html)
                                            .reply_markup(keyboard)
                                            .await
                                        {
                                            tracing::warn!("prof:back: failed to edit message: {}", e);
                                        }
                                    }
                                    return ResponseResult::Ok(());
                                }

                                let _ = bot.answer_callback_query(query.id.clone()).await;
                                return ResponseResult::Ok(());
                            }

                            // Plan Approve/Discard buttons (`plan:` prefix,
                            // deliberately distinct from tool-approval
                            // `approve:{id}`). Owner-only, same spirit as
                            // sensitive tool approval: the keyboard sits in
                            // the chat where any allowlisted member could tap
                            // it, so the tapper is re-checked here. Approve is
                            // FORBIDDEN while a turn runs (refuse, never
                            // queue); Discard cancels the turn first.
                            if data == "plan:ok" || data == "plan:no" {
                                let caller_is_owner = config_rx
                                    .borrow()
                                    .channels
                                    .telegram
                                    .is_owner(&query.from.id.0.to_string());
                                if !caller_is_owner {
                                    let _ = bot
                                        .answer_callback_query(query.id.clone())
                                        .text("🔒 Owner only")
                                        .show_alert(true)
                                        .await;
                                    return ResponseResult::Ok(());
                                }
                                let Some(session_id) =
                                    resolve_callback_session(&query, &state, &shared_session).await
                                else {
                                    let _ = bot
                                        .answer_callback_query(query.id.clone())
                                        .text("No session for this chat.")
                                        .await;
                                    return ResponseResult::Ok(());
                                };
                                let (chat_id, thread_id) = match query.message.as_ref() {
                                    Some(m) => (
                                        m.chat().id,
                                        m.regular_message().and_then(|r| r.thread_id),
                                    ),
                                    None => {
                                        let _ = bot.answer_callback_query(query.id.clone()).await;
                                        return ResponseResult::Ok(());
                                    }
                                };
                                // Used buttons disappear: clear the markup on
                                // the message that carried them (the flow tick
                                // re-attaches when the state still wants one).
                                let kb_msg_id = query.message.as_ref().map(|m| m.id());

                                if data == "plan:no" {
                                    let cancelled = state.cancel_session(session_id).await;
                                    let mut reply =
                                        crate::utils::plan_mode::discard(session_id, agent.context())
                                            .await;
                                    if cancelled {
                                        reply =
                                            format!("⏹️ Cancelled the running turn. {reply}");
                                    }
                                    let _ = bot
                                        .answer_callback_query(query.id.clone())
                                        .text("Plan discarded")
                                        .await;
                                    if let Some(mid) = kb_msg_id {
                                        let _ = bot
                                            .edit_message_reply_markup(chat_id, mid)
                                            .await;
                                    }
                                    let _ = crate::channels::telegram::send::message_in_thread(
                                        &bot, chat_id, thread_id, reply,
                                    )
                                    .await;
                                    return ResponseResult::Ok(());
                                }

                                // plan:ok — Approve / seed retry.
                                if state.is_turn_active(session_id) {
                                    let _ = bot
                                        .answer_callback_query(query.id.clone())
                                        .text(
                                            "⛔ A turn is running. Approve is refused while \
                                             busy; try again when it finishes.",
                                        )
                                        .show_alert(true)
                                        .await;
                                    return ResponseResult::Ok(());
                                }
                                match crate::utils::plan_mode::try_approve(session_id).await {
                                    crate::utils::plan_mode::ApproveOutcome::Refused(msg) => {
                                        let _ =
                                            bot.answer_callback_query(query.id.clone()).await;
                                        let _ =
                                            crate::channels::telegram::send::message_in_thread(
                                                &bot, chat_id, thread_id, msg,
                                            )
                                            .await;
                                    }
                                    crate::utils::plan_mode::ApproveOutcome::SeedTurn {
                                        prompt,
                                    } => {
                                        let _ = bot
                                            .answer_callback_query(query.id.clone())
                                            .text("✅ Plan approved")
                                            .await;
                                        if let Some(mid) = kb_msg_id {
                                            let _ = bot
                                                .edit_message_reply_markup(chat_id, mid)
                                                .await;
                                        }
                                        let _ =
                                            crate::channels::telegram::send::message_in_thread(
                                                &bot,
                                                chat_id,
                                                thread_id,
                                                "✅ Plan approved — starting now…".to_string(),
                                            )
                                            .await;
                                        // Visible seed turn, spawned so the
                                        // callback answers fast. The turn
                                        // guard keeps concurrent messages from
                                        // forking a second turn. (The /execute
                                        // COMMAND path gets full flow chrome;
                                        // the button path delivers the final
                                        // text: same engine, lighter surface.)
                                        let bot2 = bot.clone();
                                        let agent2 = agent.clone();
                                        let state2 = state.clone();
                                        tokio::spawn(async move {
                                            let _guard =
                                                match state2.try_begin_turn(session_id) {
                                                    Some(g) => g,
                                                    None => return,
                                                };
                                            let display =
                                                "[System: Plan approved — seeding checklist]"
                                                    .to_string();
                                            // MUST run the tool loop: the approval turn has to
                                            // actually call `plan` start/add_tasks and execute.
                                            // The old send_message_with_display was a single
                                            // tool-less completion, so the agent could only emit
                                            // text ("Starting now.") and the plan never started.
                                            // Lighter surface than /execute (no streaming
                                            // progress callback) but the SAME tool-enabled engine.
                                            let chat_id_str = chat_id.0.to_string();
                                            match agent2
                                                .send_message_with_tools_and_display(
                                                    session_id,
                                                    prompt,
                                                    Some(display),
                                                    None, // model
                                                    None, // cancel token
                                                    None, // approval callback
                                                    None, // progress callback (final text only)
                                                    None, // question callback
                                                    "telegram",
                                                    Some(&chat_id_str),
                                                )
                                                .await
                                            {
                                                Ok(resp) => {
                                                    let text =
                                                        crate::utils::sanitize::strip_llm_artifacts(
                                                            &resp.content,
                                                        );
                                                    let text = crate::utils::redact_secrets(&text);
                                                    if !text.trim().is_empty() {
                                                        let html = crate::channels::telegram::handler::md_to_html(&text);
                                                        let _ = crate::channels::telegram::handler::send_html_or_plain(
                                                            &bot2, chat_id, thread_id, &html,
                                                        )
                                                        .await;
                                                    }
                                                }
                                                Err(e) => {
                                                    tracing::warn!(
                                                        "Plan seed turn failed for session {session_id}: {e}"
                                                    );
                                                    let _ = crate::channels::telegram::send::message_in_thread(
                                                        &bot2,
                                                        chat_id,
                                                        thread_id,
                                                        format!(
                                                            "⚠️ Checklist seed failed: {e}. \
                                                             Retry with /execute when idle."
                                                        ),
                                                    )
                                                    .await;
                                                }
                                            }
                                        });
                                    }
                                }
                                return ResponseResult::Ok(());
                            }

                            let (approved, always, yolo, id) =
                                if let Some(id) = data.strip_prefix("approve:") {
                                    (true, false, false, id.to_string())
                                } else if let Some(id) = data.strip_prefix("always:") {
                                    (true, true, false, id.to_string())
                                } else if let Some(id) = data.strip_prefix("yolo:") {
                                    (true, true, true, id.to_string())
                                } else if let Some(id) = data.strip_prefix("deny:") {
                                    (false, false, false, id.to_string())
                                } else {
                                    tracing::warn!("Telegram: unknown callback data: {}", data);
                                    let _ = bot.answer_callback_query(query.id.clone()).await;
                                    return ResponseResult::Ok(());
                                };

                            // Persist YOLO (permanent) directly from callback
                            if yolo {
                                crate::utils::persist_auto_always_policy();
                            }

                            let resolved = state.resolve_pending_approval(&id, approved, always).await;
                            tracing::info!(
                                "Telegram approval resolved: id={}, approved={}, always={}, found_pending={}",
                                id, approved, always, resolved
                            );
                            if !resolved {
                                tracing::warn!(
                                    "Telegram: no pending approval found for id={} — may have timed out or already resolved",
                                    id
                                );
                            }
                            let _ = bot.answer_callback_query(query.id.clone()).await;

                            // Edit the approval message: keep original context, append outcome, remove buttons
                            if let Some(msg) = &query.message {
                                let label = if yolo {
                                    "\n\n🔥 YOLO — always approved"
                                } else if always {
                                    "\n\n🔁 Always approved (session)"
                                } else if approved {
                                    "\n\n✅ Approved"
                                } else {
                                    "\n\n❌ Denied"
                                };
                                let original_text = match msg {
                                    teloxide::types::MaybeInaccessibleMessage::Regular(m) => {
                                        m.text().unwrap_or("").to_string()
                                    }
                                    _ => String::new(),
                                };
                                let updated = format!("{}{}", original_text, label);
                                use teloxide::payloads::EditMessageTextSetters;
                                use teloxide::prelude::Requester;
                                if let Err(e) = bot
                                    .edit_message_text(msg.chat().id, msg.id(), &updated)
                                    .reply_markup(teloxide::types::InlineKeyboardMarkup::default())
                                    .await
                                {
                                    tracing::error!("Telegram: failed to edit approval message: {}", e);
                                }
                            } else {
                                tracing::warn!("Telegram: callback query has no message — cannot edit");
                            }
                        } else {
                            tracing::warn!("Telegram: callback query with no data");
                            let _ = bot.answer_callback_query(query.id.clone()).await;
                        }
                        ResponseResult::Ok(())
                    }
                }
            });

            // Note: service messages (member joins/leaves) are regular Message
            // updates in teloxide 0.17+ — they flow through msg_handler and are
            // captured in handler.rs BEFORE the allowlist check so bot/user IDs
            // are logged even when the joining user isn't allowlisted yet.

            // Inbound reaction handler: user reacts on a bot message, bot may
            // react back or respond with text. See handle.rs for details.
            let reaction_handler = Update::filter_message_reaction_updated().endpoint({
                let deps = deps.clone();
                move |bot: Bot, reaction: teloxide::types::MessageReactionUpdated| {
                    let deps = deps.clone();
                    async move {
                        spawn_handle_reaction(bot, reaction, deps);
                        ResponseResult::Ok(())
                    }
                }
            });

            let tree = dptree::entry()
                .branch(msg_handler)
                .branch(edited_handler)
                .branch(cb_handler)
                .branch(reaction_handler);

            // Retry loop: if the dispatcher exits (network hiccup, Telegram conflict
            // from another process using the same token, etc.), wait and reconnect.
            // Without this, daemon mode silently loses the Telegram connection forever.
            loop {
                tracing::info!("Telegram: starting dispatcher polling loop (raw-aware listener)");
                // Raw-aware listener (#354): fetches updates as raw JSON,
                // stashes each message's payload, and synthesizes readable
                // text for content types the Bot API client cannot decode
                // (forwarded rich messages), so they flow through the normal
                // pipeline. The typed parse goes through from_str ONLY —
                // from_value yields Error kinds for everything and took the
                // whole intake down once (see telegram_raw_update_parse_test).
                let listener = super::raw_updates::raw_polling_listener(listener_token.clone());
                Dispatcher::builder(bot.clone(), tree.clone())
                    .build()
                    .dispatch_with_listener(
                        listener,
                        teloxide::error_handlers::LoggingErrorHandler::with_custom_text(
                            "Telegram raw update listener error",
                        ),
                    )
                    .await;
                tracing::warn!("Telegram: dispatcher exited unexpectedly — reconnecting in 5s");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        })
    }
}

/// How long a peer bot's streamed message must go without a further edit
/// before we treat it as complete and hand the final text to the agent.
/// Peer crab bots edit their reply roughly every 1.5s while streaming
/// (matching our own stream throttle), so ~2s of quiet means the stream
/// finished — we start processing about 2s after the peer stops, which
/// keeps a bot-to-bot exchange snappy without ever acting on a partial.
const BOT_STREAM_SETTLE: Duration = Duration::from_secs(2);

/// Pending peer-bot edit streams, keyed by (chat, message). The value is
/// `(generation, latest_message)`: each new frame bumps the generation so a
/// stale settle watcher knows it was superseded and bows out.
type PendingEdits = Arc<Mutex<HashMap<(ChatId, MessageId), (u64, Message)>>>;

/// Cloneable bundle of everything `handle_message` needs, so the message,
/// edited-message, and settle paths can dispatch without threading nine
/// arguments through each.
#[derive(Clone)]
struct DispatchDeps {
    agent: Arc<AgentService>,
    session_svc: SessionService,
    bot_token: Arc<String>,
    shared_session: Arc<Mutex<Option<Uuid>>>,
    telegram_state: Arc<TelegramState>,
    config_rx: tokio::sync::watch::Receiver<Config>,
    channel_msg_repo: ChannelMessageRepository,
}

/// Should this message be held until its edit stream settles? True only for
/// a text message sent by a bot in a group — the one case where the sender
/// streams its reply via progressive edits. Humans, DMs, and non-text
/// messages process immediately.
fn is_stream_candidate(msg: &Message) -> bool {
    !msg.chat.is_private() && msg.text().is_some() && msg.from.as_ref().is_some_and(|u| u.is_bot)
}

/// Dispatch a message to the agent in the background, isolating panics so a
/// single bad turn can't take down the dispatcher. The dispatcher stays free
/// to process callback queries (approval buttons) while the agent runs.
fn spawn_handle_message(bot: Bot, msg: Message, deps: DispatchDeps) {
    tokio::spawn(async move {
        let result = tokio::task::spawn(async move {
            handle_message(
                bot,
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
        })
        .await;
        match result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::error!("Telegram handle_message error: {e}"),
            Err(panic_err) => {
                tracing::error!("Telegram handle_message panicked: {:?}", panic_err)
            }
        }
    });
}

/// Dispatch an inbound reaction to the agent in the background.
/// Same pattern as `spawn_handle_message`: isolate panics, keep dispatcher free.
fn spawn_handle_reaction(
    bot: Bot,
    reaction: teloxide::types::MessageReactionUpdated,
    deps: DispatchDeps,
) {
    tokio::spawn(async move {
        let result = tokio::task::spawn(async move {
            handle_reaction(
                bot,
                reaction,
                deps.agent,
                deps.shared_session,
                deps.telegram_state,
                deps.config_rx,
                deps.channel_msg_repo,
            )
            .await
        })
        .await;
        match result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::error!("Telegram handle_reaction error: {e}"),
            Err(panic_err) => {
                tracing::error!("Telegram handle_reaction panicked: {:?}", panic_err)
            }
        }
    });
}

/// Wait for a peer bot's edit stream to go quiet, then process the final
/// text. If a newer frame arrived while we waited (generation moved on) we
/// bow out: that frame's own watcher will fire. The matching watcher
/// removes the pending entry, so the map self-cleans.
fn spawn_settle_watcher(
    key: (ChatId, MessageId),
    generation: u64,
    pending_edits: PendingEdits,
    bot: Bot,
    deps: DispatchDeps,
) {
    tokio::spawn(async move {
        tokio::time::sleep(BOT_STREAM_SETTLE).await;
        let final_msg = {
            let mut map = pending_edits.lock().await;
            match map.get(&key) {
                Some((g, _)) if *g == generation => map.remove(&key).map(|(_, m)| m),
                _ => None,
            }
        };
        if let Some(final_msg) = final_msg {
            tracing::debug!(
                "Telegram: peer-bot message in chat {} settled after streaming — \
                 processing final text",
                key.0.0
            );
            spawn_handle_message(bot, final_msg, deps);
        }
    });
}

/// Resolve the correct session ID for a callback query.
///
/// Callbacks from inline buttons (e.g. `/models` picker) fire in the chat
/// where the button was pressed. We look up the session that's registered for
/// that chat — this is the session the message handler resolved. Only falls
/// back to the shared TUI session when no chat→session mapping exists (e.g.
/// first-ever interaction before any message was processed).
async fn resolve_callback_session(
    query: &CallbackQuery,
    state: &super::TelegramState,
    shared_session: &tokio::sync::Mutex<Option<Uuid>>,
) -> Option<Uuid> {
    // Try to get the session registered for this chat, scoped to the forum
    // topic the button was pressed in (#215) so a callback inside a topic
    // resolves that topic's session, not the base one.
    if let Some(msg) = &query.message {
        let chat_id = msg.chat().id.0;
        let topic_id = msg.regular_message().and_then(|m| {
            super::session_resolve::topic_session_id(m.is_topic_message, m.thread_id.map(|t| t.0.0))
        });
        if let Some(session_id) = state.chat_session(chat_id, topic_id).await {
            return Some(session_id);
        }
    }
    // Fallback: shared TUI session (owner DMs before any message handler ran)
    *shared_session.lock().await
}

/// Register bot commands with Telegram so they appear in the `/` menu.
///
/// Builds the command list dynamically from:
/// 1. Built-in commands (hardcoded)
/// 2. User-defined commands from commands.toml
/// 3. Skills from SKILL.md files
///
/// Telegram constraints: max 100 commands, names must be lowercase with
/// underscores only (no hyphens), descriptions max 256 chars.
pub(crate) async fn register_bot_commands(bot: &Bot) {
    use teloxide::types::BotCommand;

    let mut commands: Vec<BotCommand> = vec![
        BotCommand::new("start", "Get your user ID to start using the bot"),
        BotCommand::new("new", "Start a new session"),
        BotCommand::new("cd", "Change working directory"),
        BotCommand::new("sessions", "List and switch sessions"),
        BotCommand::new("stop", "Cancel the current operation"),
        BotCommand::new("help", "Show available commands"),
        BotCommand::new("models", "Switch AI model or provider"),
        BotCommand::new("usage", "Session token and cost stats"),
        // Authored with the canonical hyphen; sanitized to `mission_control`
        // for the menu below (Telegram command names can't contain hyphens).
        // The dispatcher and /help keep the dash; the menu chip is underscore,
        // consistent with every other multi-word command/skill.
        BotCommand::new(
            "mission-control",
            "Mission control: analytics, activity, inbox & schedule",
        ),
        BotCommand::new("compact", "Compact conversation context"),
        BotCommand::new("goal", "Set/track an autonomous goal"),
        BotCommand::new("profiles", "Manage profiles (create, switch, migrate)"),
        BotCommand::new(
            "cowork",
            "Create a cowork workspace with QR invite (Telegram only)",
        ),
        BotCommand::new("doctor", "Run connection health check"),
        BotCommand::new("evolve", "Check for updates"),
        BotCommand::new("rtk", "Show RTK token savings statistics"),
        BotCommand::new("respond_to", "Show/switch auto-mention mode"),
        BotCommand::new("plan", "Enter Plan mode (design a plan for approval)"),
        BotCommand::new("show_plan", "Show the current plan state"),
        BotCommand::new(
            "execute",
            "Approve the design plan / retry the checklist seed",
        ),
        BotCommand::new("discard", "Discard the live plan (back to no plan)"),
    ];

    // Load user-defined commands from commands.toml
    let brain_path = crate::brain::BrainLoader::resolve_path();
    let loader = crate::brain::CommandLoader::from_brain_path(&brain_path);
    let user_commands = loader.load();
    for cmd in &user_commands {
        // Strip leading slash and convert to Telegram format.
        let name = cmd.name.strip_prefix('/').unwrap_or(&cmd.name);
        let name = sanitize_command_name(name);
        if name.is_empty() {
            continue;
        }
        let description = truncate_description(&cmd.description, 256);
        commands.push(BotCommand::new(name, description));
    }

    // Load skills and register them as commands.
    let skills = crate::brain::skills::load_all_skills();
    for skill in &skills {
        // Skills use hyphens (security-audit); convert to underscores.
        let name = sanitize_command_name(&skill.name);
        if name.is_empty() {
            continue;
        }
        let description = truncate_description(&skill.description, 256);
        commands.push(BotCommand::new(name, description));
    }

    // Normalize EVERY command name to Telegram's rules ([a-z0-9_], 1-32 chars).
    // Telegram rejects the ENTIRE setMyCommands call if a single name is
    // invalid, and it can't show hyphens, so the menu standard is the
    // underscore form: `mission-control` → `mission_control`, consistent with
    // every other multi-word command and skill. The canonical dash is kept in
    // /help and accepted (alongside the underscore) by the dispatcher.
    for c in &mut commands {
        c.command = sanitize_command_name(&c.command);
    }
    commands.retain(|c| !c.command.is_empty() && c.command.len() <= 32);

    // Dedup by normalized name — built-ins are listed first, so a commands.toml
    // entry or skill that shadows a built-in (e.g. a user-defined `/goal`) is
    // dropped instead of creating a duplicate menu chip. Telegram would render
    // two identical entries otherwise.
    let mut seen = std::collections::HashSet::new();
    commands.retain(|c| seen.insert(c.command.clone()));

    // Telegram limit: max 100 commands
    commands.truncate(100);

    let count = commands.len();
    match bot.set_my_commands(commands).await {
        Ok(_) => tracing::info!("Telegram: registered {} bot commands", count),
        Err(e) => tracing::warn!("Telegram: failed to register bot commands: {}", e),
    }
}

/// Sanitize a command name for Telegram: lowercase, underscores only.
/// Telegram only allows: a-z, 0-9, and underscores.
pub(crate) fn sanitize_command_name(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c == '-' { '_' } else { c })
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}

/// Truncate description to max character length, adding ellipsis if needed.
/// Telegram limits descriptions by character count, not byte count.
pub(crate) fn truncate_description(desc: &str, max_chars: usize) -> String {
    let char_count = desc.chars().count();
    if char_count <= max_chars {
        desc.to_string()
    } else {
        let truncated: String = desc.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{truncated}…")
    }
}
