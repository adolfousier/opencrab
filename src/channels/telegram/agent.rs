//! Telegram Agent
//!
//! Agent struct and startup logic.

use super::TelegramState;
use super::handler::handle_message;
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

            // Verify token works with Telegram API before setting up dispatcher
            match bot.get_me().await {
                Ok(me) => {
                    if let Some(ref username) = me.username {
                        tracing::info!("Telegram: bot username is @{}", username);
                        self.telegram_state.set_bot_username(username.clone()).await;
                    }
                    // Store bot in state for proactive messaging only after successful auth
                    self.telegram_state.set_bot(bot.clone()).await;

                    // Register slash commands so they appear in Telegram's / menu
                    register_bot_commands(&bot).await;
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
                                    if let Ok(config) = crate::config::Config::load()
                                        && let Ok(new_provider) = crate::brain::provider::factory::create_provider_by_name(&config, provider_name).await
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
                                    let caller_id = query.from.id.0 as i64;
                                    let owner_id = cfg
                                        .channels
                                        .telegram
                                        .allowed_users
                                        .first()
                                        .and_then(|s| s.parse::<i64>().ok());
                                    let is_owner = cfg.channels.telegram.allowed_users.is_empty()
                                        || owner_id == Some(caller_id);

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
                                        .unwrap_or(caller_id);
                                    state.register_session_chat(new_id, switch_chat_id).await;

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
            let tree = dptree::entry()
                .branch(msg_handler)
                .branch(edited_handler)
                .branch(cb_handler);

            // Retry loop: if the dispatcher exits (network hiccup, Telegram conflict
            // from another process using the same token, etc.), wait and reconnect.
            // Without this, daemon mode silently loses the Telegram connection forever.
            loop {
                tracing::info!("Telegram: starting dispatcher polling loop");
                Dispatcher::builder(bot.clone(), tree.clone())
                    .build()
                    .dispatch()
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
/// (matching our own stream throttle), so a few seconds of quiet reliably
/// means the stream finished.
const BOT_STREAM_SETTLE: Duration = Duration::from_secs(4);

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
    !msg.chat.is_private()
        && msg.text().is_some()
        && msg.from.as_ref().is_some_and(|u| u.is_bot)
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

/// Wait for a peer bot's edit stream to go quiet, then process the final
/// text. If a newer frame arrived while we waited (generation moved on) we
/// bow out — that frame's own watcher will fire. The matching watcher
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
    // Try to get the session registered for this chat
    if let Some(msg) = &query.message {
        let chat_id = msg.chat().id.0;
        if let Some(session_id) = state.chat_session(chat_id).await {
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
        BotCommand::new("help", "Show available commands"),
        BotCommand::new("models", "Switch AI model or provider"),
        BotCommand::new("usage", "Session token and cost stats"),
        BotCommand::new("new", "Start a new session"),
        BotCommand::new("sessions", "List and switch sessions"),
        BotCommand::new("stop", "Cancel the current operation"),
        BotCommand::new("compact", "Compact conversation context"),
        BotCommand::new("doctor", "Run connection health check"),
        BotCommand::new("evolve", "Check for updates"),
        BotCommand::new("rtk", "Show RTK token savings statistics"),
    ];

    // Load user-defined commands from commands.toml
    let brain_path = crate::brain::BrainLoader::resolve_path();
    let loader = crate::brain::CommandLoader::from_brain_path(&brain_path);
    let user_commands = loader.load();
    for cmd in &user_commands {
        // Strip leading slash and convert to Telegram format
        let name = cmd.name.strip_prefix('/').unwrap_or(&cmd.name);
        let name = sanitize_command_name(name);
        if name.is_empty() {
            continue;
        }
        let description = truncate_description(&cmd.description, 256);
        commands.push(BotCommand::new(name, description));
    }

    // Load skills and register them as commands
    let skills = crate::brain::skills::load_all_skills();
    for skill in &skills {
        // Skills use hyphens (security-audit), convert to underscores
        let name = sanitize_command_name(&skill.name);
        if name.is_empty() {
            continue;
        }
        let description = truncate_description(&skill.description, 256);
        commands.push(BotCommand::new(name, description));
    }

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
