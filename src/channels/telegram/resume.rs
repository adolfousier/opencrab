//! Crash-recovery resume: replays an interrupted Telegram turn on startup
//! with full streaming (typing, tool messages, edit loop, final delivery).
//!
//! Moved VERBATIM out of handler.rs (#471 phase 1, pure decomposition —
//! the handler glob re-export keeps every existing call site stable).

use super::TelegramState;
#[allow(unused_imports)]
use super::handler::*;
use super::send::{best_effort_delete, fire_chat_action};
use crate::brain::agent::{AgentService, ProgressCallback, ProgressEvent};
use crate::config::Config;
use crate::db::ChannelMessageRepository;
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::ChatAction;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Build the background-task enqueue producer for Telegram (#722).
///
/// When a detached long command finishes, this resumes the session by delivering
/// a turn to its chat via `resume_session` (full streaming pipeline). The agent
/// handle can't be captured at service-creation time (it's being built), so a
/// weak holder filled afterwards breaks the cycle; on call we upgrade it.
pub(crate) fn build_enqueue_callback(
    state: Arc<TelegramState>,
    agent_holder: Arc<std::sync::Mutex<Option<std::sync::Weak<AgentService>>>>,
) -> crate::brain::agent::service::MessageEnqueueCallback {
    Arc::new(move |session_id, msg| {
        let state = state.clone();
        let agent_holder = agent_holder.clone();
        tokio::spawn(async move {
            let Some(chat_id) = state.session_chat(session_id).await else {
                tracing::warn!("[bg-resume] telegram: no chat for session {session_id}; dropping");
                return;
            };
            let Some(bot) = state.bot().await else {
                tracing::warn!("[bg-resume] telegram: bot not available; dropping resume");
                return;
            };
            let Some(agent) = agent_holder
                .lock()
                .ok()
                .and_then(|g| g.as_ref().and_then(|w| w.upgrade()))
            else {
                tracing::warn!("[bg-resume] telegram: agent gone; dropping resume");
                return;
            };
            // The topic that OWNS the session, not whichever one spoke last
            // (#1200). Sessions are per-topic since #215, and
            // `register_session_chat` records the topic, but this path asked
            // the chat instead: in a forum, any traffic in another topic while
            // a detached command ran sent the result there. Both background
            // completions and sub-agent results share this callback, so they
            // were both misrouted.
            //
            // The chat-wide lookup stays as the fallback. It is still right
            // for a DM or a non-forum group (where `session_topic` is None
            // anyway), and it is all we have for a session whose in-memory
            // topic binding has not been re-registered since a restart.
            let thread_id = match state.session_topic(session_id).await {
                Some(topic) => Some(teloxide::types::ThreadId(teloxide::types::MessageId(topic))),
                None => super::send::latest_thread_id_for_chat(chat_id).await,
            };

            // #1221: announce WHAT arrived before anything else happens — an
            // expandable blockquote echoing the completion output (rich format
            // preserved), so the user sees why the session woke up even when
            // the resumed answer restates it. Fires on BOTH delivery paths:
            // before the guard branch below means idle-wakes AND results that
            // get queued into an in-flight turn are announced alike. Covers
            // background-task completions AND session_notify pushes (#1221
            // notify lane); sub-agent pushes stay silent.
            if matches!(
                msg.origin,
                crate::brain::agent::PushOrigin::BackgroundTask
                    | crate::brain::agent::PushOrigin::SessionNotify
            ) {
                // #1225: session_notify pushes carry a mechanical
                // `[session-notify from=<uuid>]` header — replace the raw id
                // with a human label (topic name for same-chat pushes, chat
                // name / chat+topic for cross-chat, per Alexey's rule), so the
                // bubble reads "📨 From: Ops / Push to session", not a UUID
                // spray. Background-task pushes have no sender: the title
                // names the task from the producer's display line instead of
                // the generic "⚙️ background task result" (Alexey 2026-08-26).
                let (sender, body) = split_bg_echo_parts(&msg.context_text);
                let title = if let Some(s) = sender {
                    format!("📨 {}", sender_label(&state, &bot, s, chat_id).await)
                } else if msg.origin == crate::brain::agent::PushOrigin::BackgroundTask {
                    // Borrow: `msg` is moved wholesale later in this callback.
                    background_task_title(&msg.display_text)
                } else {
                    "⚙️ background task result".to_owned()
                };
                let (_, classic_html) = build_bg_echo_bubble(&body, &title);
                // Rich dialect needs the RICH envelope: `<details><summary>`
                // collapsibles (RichBlockDetails, Bot API 10.1) — the same card
                // component plan_card renders. The classic
                // `<blockquote expandable>` markup is the sendMessage dialect;
                // under sendRichMessage it renders as a flat quote, not a rich
                // card (user-verified 2026-08-26).
                let rich_html = build_bg_echo_bubble_rich(&body, &title);
                
                // Rich-first: route the echo bubble through the same canonical
                // rich send plan_card uses. Any send failure degrades to the
                // classic HTML blockquote below.
                let sent_rich = match super::rich::api::send_rich_html_id(
                    bot.api_url().as_str(),
                    bot.token(),
                    chat_id,
                    thread_id,
                    &rich_html,
                    None,
                    "bg-resume",
                    "-",
                )
                .await
                {
                    Ok(_) => true,
                    Err(e) => {
                        tracing::warn!(
                            "[bg-resume] #1225 rich echo failed, using HTML: {e}"
                        );
                        false
                    }
                };
                if !sent_rich {
                    let mut echo = bot
                        .send_message(teloxide::types::ChatId(chat_id), classic_html.clone())
                        .parse_mode(teloxide::types::ParseMode::Html);
                    // Forum topics address a thread; DMs and non-forum groups must
                    // omit the parameter entirely (E0308: not an unwrap decision).
                    if let Some(tid) = thread_id {
                        echo = echo.message_thread_id(tid);
                    }
                    // 429 discipline (#816): the raw send_message used to race
                    // the resumed stream + typing loop into the same chat and
                    // drop silently — 11/11 real echo sends failed that way on
                    // 2026-08-26 (per-chat flood windows, compiler batch log).
                    // Wait the window out (shared wait_out policy) and retry ONCE
                    // with fresh content, matching delivery.rs / flow.rs.
                    match echo.await {
                        Ok(_) => {}
                        Err(teloxide::RequestError::RetryAfter(secs)) => {
                            super::rate_limit::wait_out(
                                "bg-resume echo",
                                secs.duration(),
                                " on first delivery, retrying once",
                            )
                            .await;
                            let mut retry = bot
                                .send_message(teloxide::types::ChatId(chat_id), classic_html)
                                .parse_mode(teloxide::types::ParseMode::Html);
                            if let Some(tid) = thread_id {
                                retry = retry.message_thread_id(tid);
                            }
                            if let Err(e) = retry.await {
                                tracing::warn!(
                                    "[bg-resume] #1221 echo bubble failed on 429 retry: {e}"
                                );
                            }
                        }
                        Err(e) => {
                            tracing::warn!("[bg-resume] #1221 echo bubble failed to send: {e}");
                        }
                    }
                }
            }

            // One streaming turn per session (#845). Several detached commands
            // finishing together used to spawn a resume turn each, all on this
            // session and all in this chat, so every one opened and edited its
            // own flow block: duplicated, overlapping output.
            //
            // The background path was the only one skipping this gate; user
            // messages and reactions have gone through it since #501.
            let Some(_turn_guard) = state.try_begin_turn(session_id) else {
                // A turn is already streaming. Hand the result to it instead of
                // forking a second one: the tool loop drains this queue between
                // rounds via reaction_queue_callback, so the result lands in the
                // block that is already open.
                tracing::info!(
                    "[bg-resume] telegram: session {session_id} already streaming — queuing the \
                     result for the in-flight turn instead of opening a second block"
                );
                // Tagged as detached work, not as a reaction (#1213). The
                // guard covers the whole delivery window, not just the tool
                // loop, so "already streaming" can mean the loop has finished
                // and only the final bubble is still going out — in which case
                // nothing will drain this between rounds and the end-of-turn
                // flush has to know it needs a real tool loop, not a single
                // toolless round.
                state.enqueue_detached_result(session_id, msg);
                return;
            };

            // The topic that OWNS the session, not whichever one spoke last
            // (#1200). Sessions are per-topic since #215, and
            // `register_session_chat` records the topic, but this path asked
            // the chat instead: in a forum, any traffic in another topic while
            // a detached command ran sent the result there. Both background
            // completions and sub-agent results share this callback, so they
            // were both misrouted.
            //
            // The chat-wide lookup stays as the fallback. It is still right
            // for a DM or a non-forum group (where `session_topic` is None
            // anyway), and it is all we have for a session whose in-memory
            // topic binding has not been re-registered since a restart.
            let thread_id = match state.session_topic(session_id).await {
                Some(topic) => Some(teloxide::types::ThreadId(teloxide::types::MessageId(topic))),
                None => super::send::latest_thread_id_for_chat(chat_id).await,
            };
            if let Err(e) = resume_session_inner(
                bot,
                teloxide::types::ChatId(chat_id),
                thread_id,
                session_id,
                msg.context_text,
                agent,
                state,
            )
            .await
            {
                tracing::warn!("[bg-resume] telegram resume_session failed: {e}");
            }
        });
    })
}

/// Public entry for resuming a session with the full streaming pipeline:
/// claims the session's turn slot (#1222), then drives `resume_session_inner`.
///
/// Callers that ALREADY hold the turn guard — the bg-resume enqueue callback
/// (resume.rs #845/#1213), the end-of-turn stranded flush (handler.rs #1213)
/// and the agent approval flows (agent.rs) — must call `resume_session_inner`
/// directly so the slot is not double-claimed.
///
/// Called from ui.rs on startup when pending Telegram requests are detected.
pub(crate) async fn resume_session(
    bot: Bot,
    chat_id: ChatId,
    thread_id: Option<teloxide::types::ThreadId>,
    session_id: Uuid,
    prompt: String,
    agent: Arc<AgentService>,
    telegram_state: Arc<TelegramState>,
) -> anyhow::Result<()> {
    // Claim the session's turn slot for the whole replay (#1222). A recovery
    // replay drives the SAME edit loop as an ingress turn but used to run
    // without holding TelegramState's active_turns flag, so any inbound
    // message arriving mid-replay read the session as idle and forked a
    // second concurrent tool loop on top of it — two interleaved streaming
    // blocks progressing in one topic. Holding the RAII guard for the whole
    // resumed turn makes ingress queue follow-ups (#501/#845) exactly as it
    // does for normal turns; resume-born loops already drain those queues.
    // If the slot is somehow taken, skipping is safe: the pending item was
    // already cleared from the repo and something else owns the turn.
    let _turn_guard = match telegram_state.try_begin_turn(session_id) {
        Some(guard) => guard,
        None => {
            tracing::warn!(
                "Telegram: resume_session {session_id} skipped — a turn is already active for this session"
            );
            return Ok(());
        }
    };
    resume_session_inner(
        bot,
        chat_id,
        thread_id,
        session_id,
        prompt,
        agent,
        telegram_state,
    )
    .await
}

/// Unguarded core of `resume_session`. The turn-slot contract lives in the
/// caller: either hold an `ActiveTurnGuard` across the await, or go through
/// the public wrapper.
pub(crate) async fn resume_session_inner(
    bot: Bot,
    chat_id: ChatId,
    thread_id: Option<teloxide::types::ThreadId>,
    session_id: Uuid,
    prompt: String,
    agent: Arc<AgentService>,
    telegram_state: Arc<TelegramState>,
) -> anyhow::Result<()> {
    tracing::info!(
        "Telegram: resume_session {} with full streaming pipeline",
        session_id
    );

    // ── Typing indicator ────────────────────────────────────────────────────
    let typing_cancel = CancellationToken::new();
    let _typing_guard = TypingGuard(typing_cancel.clone());
    // Outlives the turn while the session still has detached work, so a long
    // background command does not leave the chat looking dead (#812).
    super::typing::spawn_typing(
        bot.clone(),
        chat_id,
        thread_id,
        typing_cancel.clone(),
        agent.background_manager(),
        session_id,
    );

    // ── Streaming setup ────────────────────────────────────────────────────
    let streaming = Arc::new(std::sync::Mutex::new(StreamingState {
        // Telegram: positive chat id = private/DM, negative = group (#677).
        is_dm: chat_id.0 > 0,
        pending_suggestions: None,
        msg_id: None,
        thinking: String::new(),
        tool_msgs: Vec::new(),
        display_queue: Vec::new(),
        open_group_msg_id: None,
        flow_entries: Vec::new(),
        flow_status: None,
        flow_rich: false,
        response: String::new(),
        final_bubble: None,
        dirty: false,
        recreate: false,
        header_preview: None,
        sections: Default::default(),
        retained_goal: None,
        applied_plan_kb: Default::default(),
        tool_round_count: 0,
        tools_started_at: Some(std::time::Instant::now()),
        turn_started_at: std::time::Instant::now(),
        flow_outcome: None,
        bg_indicator: None,
        bg_count: None,
        subagent_counts: Default::default(),
        sent_intermediates: Vec::new(),
        intermediate_msg_ids: Vec::new(),
        voice_msg_ids: Vec::new(),
        processing: true,
        // Cap folded narration only for CLI providers (#532).
        is_cli: agent.provider_for_session(session_id).cli_handles_tools(),
    }));

    let edit_cancel = CancellationToken::new();

    // Edit loop — same as handle_message
    // Store JoinHandle to await after cancellation (prevents duplicate race).
    // The resumed turn drives the SAME streaming edit loop as handle_message
    // (#1086 seam 5). This file used to carry a second implementation that
    // drifted from the original: it missed the tool-message edit pass, the
    // settle-flow handling and the reasoning excerpt, while the original
    // missed the placeholder-send telemetry this one had. Both gaps close by
    // sharing one loop. A resumed turn has no inbound message, so there is
    // nothing to react to (#261).
    let edit_loop_handle = super::stream_loop::spawn_edit_loop(
        &bot,
        chat_id,
        None,
        thread_id,
        chat_id.0 > 0,
        &streaming,
        &edit_cancel,
        &telegram_state,
        &agent,
        session_id,
    );

    // Progress callback — same as handle_message
    let progress_cb: ProgressCallback = {
        let st = streaming.clone();
        let bot_typing = bot.clone();
        let chat_typing = chat_id;
        Arc::new(move |_sid, event| match event {
            // Auto-compaction silent window — immediate typing refresh.
            // See handle_message for the full rationale.
            ProgressEvent::Compacting => {
                let bot = bot_typing.clone();
                let chat = chat_typing;
                tokio::spawn(async move {
                    fire_chat_action(&bot, chat, thread_id, ChatAction::Typing, "resume typing")
                        .await;
                });
            }
            ProgressEvent::ReasoningChunk { text } => {
                if let Ok(mut s) = st.lock() {
                    s.thinking.push_str(&text);
                    s.dirty = true;
                }
            }
            ProgressEvent::StreamingChunk { text } => {
                if let Ok(mut s) = st.lock() {
                    if !s.thinking.is_empty() {
                        s.thinking.clear();
                    }
                    s.response.push_str(&text);
                    s.dirty = true;
                    s.processing = false;
                }
            }
            ProgressEvent::ToolStarted {
                tool_name,
                tool_input,
            } => {
                if let Ok(mut s) = st.lock() {
                    s.thinking.clear();
                    if s.tools_started_at.is_none() {
                        s.tools_started_at = Some(std::time::Instant::now());
                    }
                    let ctx = tool_context(&tool_name, &tool_input);
                    let raw_ctx = crate::utils::tool_status_source(&tool_name, &tool_input);
                    let idx = s.tool_msgs.len();
                    s.tool_msgs.push(ToolMsg {
                        msg_id: None,
                        name: tool_name,
                        context: ctx,
                        raw_context: raw_ctx,
                        completed: None,
                        dirty: true,
                    });
                    s.display_queue.push(DisplayItem::NewTool(idx));
                }
            }
            ProgressEvent::ToolCompleted {
                tool_name, success, ..
            } => {
                if let Ok(mut s) = st.lock() {
                    s.tool_round_count += 1;
                    if let Some(tool) = s
                        .tool_msgs
                        .iter_mut()
                        .rev()
                        .find(|t| t.name == tool_name && t.completed.is_none())
                    {
                        tool.completed = Some(success);
                        tool.dirty = true;
                    }
                    // No recreate here (#299) — see the handle_message arm:
                    // completions edit the group in place, nothing new lands
                    // below the placeholder.
                }
            }
            ProgressEvent::QueuedUserMessage { .. } => {
                detach_flow_for_followup(&st);
            }
            ProgressEvent::IntermediateText { text, reasoning: _ } => {
                if let Ok(mut s) = st.lock() {
                    s.thinking.clear();
                    s.response.clear();
                    if s.msg_id.is_some() {
                        s.recreate = true;
                    }
                    // Never push reasoning as a standalone intermediate — it
                    // belongs in the streaming response's 💭 thinking block.
                    // Using reasoning as a fallback here causes duplicate
                    // messages on Telegram (reasoning intermediate + final
                    // response that doesn't contain the reasoning text, so
                    // dedup can't strip it).
                    if !text.is_empty() {
                        s.display_queue.push(DisplayItem::Intermediate(text));
                    }
                }
            }
            ProgressEvent::SelfHealingAlert { message } => {
                if let Ok(mut s) = st.lock() {
                    s.display_queue
                        .push(DisplayItem::Intermediate(format!("🔧 {}", message)));
                }
            }
            ProgressEvent::RetryAttempt {
                attempt,
                max,
                reason,
            } => {
                if let Ok(mut s) = st.lock() {
                    s.display_queue.push(DisplayItem::Intermediate(format!(
                        "⏳ Retry {}/{} — {}",
                        attempt, max, reason
                    )));
                }
            }
            ProgressEvent::ProviderSwitched {
                to_name, to_model, ..
            } => {
                if let Ok(mut s) = st.lock() {
                    s.display_queue.push(DisplayItem::Intermediate(format!(
                        "🔄 Now using {}/{}",
                        to_name, to_model
                    )));
                }
            }
            ProgressEvent::SuggestedOptions(options) => {
                // Stash only (#724): buttons must be the FINAL message in the
                // chat, so render_suggestions runs after deliver_final_response
                // below — where it can also merge into the answer bubble
                // (#tg-suggest-merge). The old mid-stream spawn posted the
                // keyboard ABOVE the final answer on resumed turns.
                if let Ok(mut s) = st.lock() {
                    s.pending_suggestions = Some(options);
                }
            }
            _ => {}
        })
    };

    // ── Agent call ──────────────────────────────────────────────────────────
    let cancel_token = CancellationToken::new();
    telegram_state
        .store_cancel_token(session_id, cancel_token.clone())
        .await;

    let chat_id_str = chat_id.0.to_string();
    let result = agent
        .resume_interrupted_turn(
            session_id,
            prompt,
            None,
            Some(cancel_token.clone()),
            None, // no approval callback for resume
            Some(progress_cb),
            "telegram",
            Some(&chat_id_str),
        )
        .await;

    telegram_state.remove_cancel_token(session_id).await;
    edit_cancel.cancel();
    // Await edit loop to prevent race where it sends a NEW message after
    // we grab streaming_msg_id (causes duplicate completion).
    if let Err(e) = edit_loop_handle.await {
        tracing::warn!(error = %e, "Telegram resume edit loop task panicked");
    }

    // ── Final delivery ─────────────────────────────────────────────────────
    let (streaming_msg_id, remaining_display) = {
        let mut s = streaming.lock().unwrap_or_else(|e| e.into_inner());
        let display: Vec<DisplayItem> = std::mem::take(&mut s.display_queue);
        (s.msg_id, display)
    };

    if cancel_token.is_cancelled() {
        tracing::info!(
            "Telegram: resume for session {} cancelled by new message",
            session_id
        );
        // Only delete the streaming placeholder — keep prior
        // intermediate + tool-call history visible. See the matching
        // block in handle_message() for rationale.
        if let Some(mid) = streaming_msg_id {
            best_effort_delete(&bot, chat_id, mid, "streaming teardown").await;
        }
        return Ok(());
    }

    // Send remaining display items through the ONE shared drain (#470).
    // Resume has no inbound message to react to.
    drain_remaining_display(
        &bot,
        chat_id,
        thread_id,
        &streaming,
        remaining_display,
        None,
    )
    .await;

    // ── Final response ────────────────────────────────────────────────────
    // ONE delivery path (#471 phase 3): resume rides the exact same
    // deliver_final_response as live turns. Deliberate unifications vs the
    // old copy, all previously hand-maintained drift:
    // - the ctx footer renders in <i> like live turns (was <sub> here)
    // - the intermediate dedup uses the shared normalized matching
    // - group history records the bot reply (the old copy skipped it)
    // inbound=None: the original message id is lost across restarts, so
    // reactions strip without firing and reply anchoring is skipped —
    // identical to the old resume behavior.
    let voice_config = Config::current().voice_config();
    let channel_msg_repo = ChannelMessageRepository::new(agent.context().pool().clone());
    let is_dm = chat_id.0 > 0;
    // Settled header outcome for the flow block (#480), same classification
    // as the main handler: computed before `result` moves into delivery,
    // stamped after so it renders on the block's final shape.
    let flow_outcome = match &result {
        Ok(_) => FlowOutcome::Finished,
        Err(e) => {
            let es = e.to_string().to_lowercase();
            if es.contains("timed out") || es.contains("timeout") || es.contains("deadline") {
                FlowOutcome::TimedOut
            } else {
                FlowOutcome::Failed
            }
        }
    };
    if !super::handler::deliver_final_response(
        &bot,
        chat_id,
        None,
        thread_id,
        &streaming,
        session_id,
        &agent,
        &telegram_state,
        &channel_msg_repo,
        &voice_config,
        false,
        is_dm,
        "unknown",
        streaming_msg_id,
        result,
    )
    .await?
    {
        return Ok(());
    }

    // Resume parity with the main handler: settle the flow header once the
    // final delivery has left the block in its final shape.
    {
        let mut s = streaming.lock().unwrap_or_else(|e| e.into_inner());
        s.flow_outcome = Some(flow_outcome);
        let (bg_indicator, bg_count) = super::handler::bg_indicator_for(&agent, session_id);
        s.bg_indicator = bg_indicator;
        s.bg_count = bg_count;
        // Sub-agent counts ride the same settle stamp as the crash-resume
        // path's background tasks (#1183 parity with handle_message).
        s.subagent_counts = super::handler::subagent_counts_for(&agent, session_id);
    }
    // Recompute sections at settle so the plan Approve/Discard keyboard, which
    // attaches only at turn end (#571), materializes on the final render — the
    // main handler does the same right after stamping the outcome.
    super::flow_chrome::refresh_sections(&streaming, &agent, session_id).await;
    // Crash-resume settle render (#1211 G2 Final): queued, never dropped.
    refresh_flow(&bot, chat_id, &streaming, super::governor::EditClass::Final).await;
    // Settle the plan card too (#580): final checklist state + the Approve/
    // Discard keyboard, which is now gated to turn end on the card.
    let plan_kb = {
        streaming
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sections
            .plan_kb
    };
    super::plan_card::refresh_plan_card(
        &bot,
        chat_id,
        thread_id,
        &telegram_state,
        &agent,
        session_id,
        plan_kb,
    )
    .await;

    // Render buffered follow-up suggestions LAST (#724), same as handle_message:
    // buttons must be the final message in the chat, and a tap always resolves
    // to a live entry (#723). The merge host comes from deliver_final_response's
    // capture (#tg-suggest-merge) — attaching the keyboard to the answer bubble
    // kills the separate suggestions bubble on resumed turns too.
    let suggestions = streaming
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .pending_suggestions
        .take();
    if let Some(options) = suggestions {
        let merge_host = {
            let mut s = streaming.lock().unwrap_or_else(|e| e.into_inner());
            s.final_bubble.take()
        };
        super::suggest_options::render_suggestions(
            &bot,
            &telegram_state,
            session_id,
            chat_id,
            thread_id,
            options,
            merge_host,
        )
        .await;
    }

    Ok(())
}

/// Cap for the #1221 echo body: classic `sendMessage` caps a message at 4096
/// chars; header, tags and Telegram's own margin eat the rest of the budget.
const BG_ECHO_BODY_CAP_CHARS: usize = 3200;

/// Split the mechanical `[session-notify from=<uuid>]` header (#1203) off a
/// push's context text. Absent or malformed header → `(None, whole text)`.
pub(crate) fn split_notify_header(context_text: &str) -> (Option<Uuid>, &str) {
    let trimmed = context_text.trim_start();
    let Some(after_open) = trimmed.strip_prefix("[session-notify from=") else {
        return (None, trimmed);
    };
    let Some(close) = after_open.find(']') else {
        return (None, trimmed);
    };
    let rest = after_open[close + 1..].trim_start();
    match Uuid::parse_str(&after_open[..close]) {
        Ok(sender) => (Some(sender), rest),
        Err(_) => (None, trimmed),
    }
}

/// Strip the synthetic `[System: ...]` framing (constructors in
/// `brain/agent/service` terminate the block with `]`). Any other shape
/// passes through untouched.
#[cfg(test)]
pub(crate) fn strip_system_framing(text: &str) -> &str {
    let trimmed = text.trim();
    trimmed
        .strip_prefix("[System:")
        .and_then(|s| s.strip_suffix(']'))
        .map(str::trim)
        .unwrap_or(trimmed)
}

/// Split a push's text into `(sender, clean_body)`: notify header and System
/// framing both removed. `sender` is `None` for plain background-task pushes.
///
/// The push body is producer-controlled scaffolding around user-facing
/// content: a `[System: ...]` block opened at the start of the body is the
/// frame (often multi-line, ending in `]`), and everything after the block
/// is the real payload. Strip the wrapper, keep inner content + tail (#1225
/// leftover: the squash shipped tests asserting this shape but the old
/// whole-string-only strip could never satisfy them).
pub(crate) fn split_bg_echo_parts(context_text: &str) -> (Option<Uuid>, String) {
    let (sender, rest) = split_notify_header(context_text);
    (sender, strip_push_scaffolding(rest))
}

/// Aggressive scaffolding strip for push bodies: if the text opens with a
/// `[System:` block that terminates with `]`, drop the wrapper and join the
/// inner content with whatever follows. Unterminated blocks pass through
/// whole (cannot tell frame from payload). Contrast [`strip_system_framing`],
/// the conservative variant that only unwraps a whole-string wrap.
fn strip_push_scaffolding(rest: &str) -> String {
    let trimmed = rest.trim_start();
    match trimmed.strip_prefix("[System:") {
        Some(s) => match s.find(']') {
            Some(end) => {
                let inner = s[..end].trim();
                let tail = s[end + 1..].trim();
                if tail.is_empty() {
                    inner.to_owned()
                } else {
                    format!("{inner}\n{tail}")
                }
            }
            None => rest.to_owned(),
        },
        None => rest.to_owned(),
    }
}

/// Assemble the echo bubble from the clean body + title. Returns the
/// rich-capable markdown and the classic HTML blockquote fallback. Raw text
/// is truncated BEFORE conversion so the wrapper tags stay well-formed —
/// cutting rendered HTML can split a tag and make Telegram strip the
/// formatting entirely (plan_card lesson).
pub(crate) fn build_bg_echo_bubble(body: &str, title: &str) -> (String, String) {
    let truncated = body.chars().count() > BG_ECHO_BODY_CAP_CHARS;
    let body = crate::utils::string::truncate_chars(body, BG_ECHO_BODY_CAP_CHARS);
    let suffix = if truncated { " (truncated)" } else { "" };
    let markdown = format!("{title}{suffix}\n\n{body}");
    // The title is dynamic (sender label / task display line): escape it for
    // the HTML dialect so a `<` in a label can't corrupt the wrapper.
    let html = format!(
        "<blockquote expandable><b>{}{}</b>\n{}</blockquote>",
        super::markdown::escape_html(title),
        suffix,
        super::rich::markdown_to_html(body),
    );
    (markdown, html)
}

/// RICH dialect envelope for the echo bubble: `<details><summary>` collapsible
/// — the exact card component plan_card renders via `send_rich_html_id`
/// (Bot API 10.1 RichBlockDetails). The classic
/// `<blockquote expandable>` markup from [`build_bg_echo_bubble`] belongs to
/// the sendMessage dialect and renders flat under `sendRichMessage`.
/// Mirrors plan_card.rs `CollapsibleStyle::DetailsSummary`:
/// `<details><summary><b>{title}</b></summary>{body}</details>`.
pub(crate) fn build_bg_echo_bubble_rich(body: &str, title: &str) -> String {
    let truncated = body.chars().count() > BG_ECHO_BODY_CAP_CHARS;
    let body = crate::utils::string::truncate_chars(body, BG_ECHO_BODY_CAP_CHARS);
    let suffix = if truncated { " (truncated)" } else { "" };
    format!(
        "<details><summary><b>{}{}</b></summary>{}</details>",
        super::markdown::escape_html(title),
        suffix,
        super::rich::markdown_to_html(body),
    )
}

/// Bubble header for a background-task echo: reuse the producer's display
/// line, which already names the task ("🔧 background task finished:
/// <label>") — the generic "⚙️ background task result" said nothing about
/// which task woke the session (Alexey 2026-08-26). Blank display → generic
/// fallback; overlong labels are capped so the header stays readable.
pub(crate) fn background_task_title(display_text: &str) -> String {
    let t = display_text.trim();
    if t.is_empty() {
        "⚙️ background task result".to_owned()
    } else {
        crate::utils::string::truncate_chars(t, 120).to_owned()
    }
}

/// Human-readable sender label for a session_notify push (Alexey's rule):
/// sender chat == recipient chat → the sender's topic name; a different
/// non-forum chat → chat name; a different forum → chat + topic name.
/// Best-effort: any lookup failure falls back to the short session id
/// (the same prefix `session_search list` displays).
async fn sender_label(
    state: &TelegramState,
    bot: &teloxide::Bot,
    sender: Uuid,
    recipient_chat: i64,
) -> String {
    let sender_chat = state.session_chat(sender).await;
    let sender_topic = state.session_topic(sender).await;
    let api_url = bot.api_url().to_string();
    let token = bot.token().to_owned();
    let label = match sender_chat {
        None => None,
        Some(sc) if sc == recipient_chat => match sender_topic {
            Some(tid) => {
                super::titles::topic_title(
                    &api_url,
                    &token,
                    teloxide::types::ChatId(sc),
                    teloxide::types::ThreadId(teloxide::types::MessageId(tid)),
                )
                .await
            }
            None => None,
        },
        Some(sc) => {
            let chat =
                super::titles::chat_title(&api_url, &token, teloxide::types::ChatId(sc)).await;
            match (chat, sender_topic) {
                (Some(c), Some(tid)) => {
                    match super::titles::topic_title(
                        &api_url,
                        &token,
                        teloxide::types::ChatId(sc),
                        teloxide::types::ThreadId(teloxide::types::MessageId(tid)),
                    )
                    .await
                    {
                        Some(t) => Some(format!("{c} / {t}")),
                        None => Some(c),
                    }
                }
                (Some(c), None) => Some(c),
                (None, _) => None,
            }
        }
    };
    label.unwrap_or_else(|| short_session_id(sender))
}

/// Short session id: first 8 hex chars of the uuid (matches `session_search`
/// list's short-id prefix display).
fn short_session_id(uuid: Uuid) -> String {
    uuid.simple().to_string()[..8].to_owned()
}
