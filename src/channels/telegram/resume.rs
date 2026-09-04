//! Crash-recovery resume: replays an interrupted Telegram turn on startup
//! with full streaming (typing, tool messages, edit loop, final delivery).
//!
//! Moved VERBATIM out of handler.rs (#471 phase 1, pure decomposition —
//! the handler glob re-export keeps every existing call site stable).

use super::TelegramState;
#[allow(unused_imports)]
use super::handler::*;
use super::send::{best_effort_delete, fire_chat_action};
use crate::a2a::handler::notify::CLI_SENDER_PREFIX;
use crate::brain::agent::service::background_tasks;
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
            // Channel-ownership guard (fork #17): this callback is ALSO
            // reached by paths that bypass deliver_to_session's gate —
            // background-task completions resolve their route directly
            // (background_tasks.rs) — so the choke point checks too. A
            // session replaced on its chat/topic must never be woken into
            // the successor's conversation; refuse the wake. (Port seam:
            // the fork parks the message here; upstream has no channel
            // park primitive in this callback, so the completion is dropped
            // with a loud warn — the gate's contract is refusing the wake,
            // not preserving the message.)
            if let crate::brain::agent::service::session_routes::ChannelOwnership::Occupied {
                occupant,
            } = state.channel_ownership_of(session_id)
            {
                tracing::warn!(
                    "[bg-resume] telegram: session {session_id} no longer owns chat {chat_id} — \
                     occupied by session {occupant}; refusing to wake it into the successor's \
                     conversation"
                );
                return;
            }
            // Bounded wait, not a drop (#1242). At boot this callback and
            // the bot's own connect run concurrently with nothing ordering
            // them, so "not connected" here usually means "not connected
            // yet" — and answering it with a return lost the wake forever.
            let Some(bot) =
                crate::channels::transport_ready::await_transport("telegram", session_id, || {
                    state.bot()
                })
                .await
            else {
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
            // Bound sessions resolve through the delivery boundary, so a
            // General-bound one yields NO thread rather than the synthetic 1
            // (#1319). Note the two arms mean different things: bound-to-
            // General is a definite "no thread", while unbound falls back to
            // the chat-wide lookup. Collapsing them would send a General
            // session's message into whichever topic spoke last.
            let thread_id = match state.session_topic(session_id).await {
                Some(topic) => super::session_resolve::delivery_thread_id(Some(topic)),
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
                // #1377: a BARE background-task ack (typed bg_meta card) folds
                // into the session's settled flow card — one line in the
                // collapsible block, header counter re-stamped from the live
                // registry — instead of a standalone bubble. Notify pushes
                // keep their N4 document card (prose/tables ARE the content);
                // no-meta redeliveries (#1242) and cardless sessions keep the
                // bubble path below.
                let folded = if msg.origin == crate::brain::agent::PushOrigin::BackgroundTask {
                    match msg.bg_meta.clone() {
                        Some(meta) => {
                            fold_bg_ack_into_flow_card(
                                &bot,
                                chat_id,
                                &state,
                                &agent,
                                session_id,
                                &meta,
                            )
                            .await
                        }
                        None => false,
                    }
                } else {
                    false
                };
                if !folded {
                // #15: receipt cards. A bg completion carries the typed
                // payload (icon/label/duration/tail) in `bg_meta` — the card
                // renders from it, never by parsing the `[System: ...]`
                // context text. Notify pushes carry no meta: the card is
                // built from the sender label + markdown body (N4 shape).
                // #1225: session_notify pushes carry a mechanical
                // `[session-notify from=<uuid>]` header — the raw id is
                // replaced with a human label (topic name for same-chat
                // pushes, chat name / chat+topic for cross-chat, per
                // Alexey's rule). The CLI lane (#1258) stamps
                // `from=cli:<label>` instead — no sender session exists,
                // so the carried label renders verbatim, zero lookups.
                let (echo_md, classic_html) = if let Some(meta) = msg.bg_meta.clone() {
                    build_bg_receipt_card(&meta)
                } else {
                    let (sender, body) = split_bg_echo_parts(&msg.context_text);
                    match sender {
                        Some(NotifySender::Session(s)) => {
                            let label = sender_label(&state, &bot, s, chat_id).await;
                            build_notify_receipt_card(&label, &body)
                        }
                        Some(NotifySender::CliTooling(label)) => {
                            build_notify_receipt_card(label, &body)
                        }
                        // Defensive: a BackgroundTask push without meta
                        // (parked by an older binary, #1242 redelivery) keeps
                        // the #1234 flat bold-title shape. Borrow note:
                        // `msg` is moved wholesale later in this callback.
                        None if msg.origin == crate::brain::agent::PushOrigin::BackgroundTask => {
                            let title = background_task_title(&msg.display_text);
                            build_bg_echo_bubble(&body, &title)
                        }
                        None => build_bg_echo_bubble(&body, "⚙️ background task result"),
                    }
                };
                // #1234/#15: the card rides the CANONICAL markdown→rich
                // outbox (the same pipeline as every cron/kanal message);
                // the native-rich route renders the <details> collapse, the
                // fenced tail and pipe tables server-side. The classic
                // blockquote above is the degraded-path source: computed up
                // front, only touched if the outbox send fails.
                let sent_rich = match super::send::send_markdown_outbox(
                    &bot,
                    teloxide::types::ChatId(chat_id),
                    thread_id,
                    &echo_md,
                    "bg-resume",
                    "-",
                    None,
                )
                .await
                {
                    Ok(_) => true,
                    Err(e) => {
                        tracing::warn!("[bg-resume] #1234 rich echo failed, using HTML: {e}");
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
                } // #1377: end of the not-folded (standalone bubble) lane
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
                // Framing for mid-flight drains (fork #13): the queued push
                // arrives in the receiver's context as a bare user turn,
                // indistinguishable from a fresh instruction — the confusion
                // the interrupt gate's true-branch knowingly accepts. One
                // plain string tells the receiver to re-anchor after reading.
                let mut msg = msg;
                msg.context_text = format!(
                    "[queued while you were working — re-anchor to your current task after \
                     reading this]\n\n{}",
                    msg.context_text
                );
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
            // Bound sessions resolve through the delivery boundary, so a
            // General-bound one yields NO thread rather than the synthetic 1
            // (#1319). Note the two arms mean different things: bound-to-
            // General is a definite "no thread", while unbound falls back to
            // the chat-wide lookup. Collapsing them would send a General
            // session's message into whichever topic spoke last.
            let thread_id = match state.session_topic(session_id).await {
                Some(topic) => super::session_resolve::delivery_thread_id(Some(topic)),
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
                true, // push-initiated wake (#12): track for restart recovery
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
#[allow(clippy::too_many_arguments)]
pub(crate) async fn resume_session(
    bot: Bot,
    chat_id: ChatId,
    thread_id: Option<teloxide::types::ThreadId>,
    session_id: Uuid,
    prompt: String,
    agent: Arc<AgentService>,
    telegram_state: Arc<TelegramState>,
    track_push_turn: bool,
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
        track_push_turn,
    )
    .await
}

/// Unguarded core of `resume_session`. The turn-slot contract lives in the
/// caller: either hold an `ActiveTurnGuard` across the await, or go through
/// the public wrapper.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn resume_session_inner(
    bot: Bot,
    chat_id: ChatId,
    thread_id: Option<teloxide::types::ThreadId>,
    session_id: Uuid,
    prompt: String,
    agent: Arc<AgentService>,
    telegram_state: Arc<TelegramState>,
    track_push_turn: bool,
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
        compacting: false,
        pending_suggestions: None,
        pending_trailer: None,
        msg_id: None,
        thinking: String::new(),
        tool_msgs: Vec::new(),
        display_queue: Vec::new(),
        open_group_msg_id: None,
        rich_transport_failures: 0,
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
            // Auto-compaction silent window — immediate typing refresh plus
            // the visible start line in the flow body (#29).
            // See handle_message for the full rationale.
            ProgressEvent::Compacting {
                usage_pct,
                predicted,
            } => {
                let bot = bot_typing.clone();
                let chat = chat_typing;
                tokio::spawn(async move {
                    fire_chat_action(&bot, chat, thread_id, ChatAction::Typing, "resume typing")
                        .await;
                });
                if let Ok(mut s) = st.lock() {
                    s.compacting = true;
                    s.header_preview = Some(COMPACTING_HEADER_TEXT.to_string());
                    s.display_queue
                        .push(DisplayItem::Intermediate(compacting_flow_line(
                            usage_pct, predicted,
                        )));
                }
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
                        .push(DisplayItem::System(format!("🔧 {}", message)));
                }
            }
            ProgressEvent::RetryAttempt {
                attempt,
                max,
                reason,
            } => {
                if let Ok(mut s) = st.lock() {
                    s.display_queue.push(DisplayItem::System(format!(
                        "⏳ Retry {}/{} — {}",
                        attempt, max, reason
                    )));
                }
            }
            ProgressEvent::ProviderSwitched {
                to_name, to_model, ..
            } => {
                if let Ok(mut s) = st.lock() {
                    s.display_queue.push(DisplayItem::System(format!(
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
            // Compaction finished — definitive completion receipt (#29).
            // See the handle_message progress callback for the rationale.
            ProgressEvent::CompactionSummary {
                before_pct,
                after_pct,
                elapsed,
                ..
            } => {
                if let Ok(mut s) = st.lock() {
                    s.compacting = false;
                    s.header_preview = None;
                    s.display_queue
                        .push(DisplayItem::Intermediate(compacted_flow_line(
                            before_pct, after_pct, elapsed,
                        )));
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
    let result = if track_push_turn {
        // Push-initiated wake (bg-resume completion, stranded flush, boot
        // re-delivery): tracked with origin `system` so a kill mid-tool
        // leaves a boot-visible row (#12). The prompt IS the original push
        // text for push wakes, so it persists correctly.
        agent
            .send_push_turn(
                session_id,
                prompt,
                None,
                Some(cancel_token.clone()),
                None, // no approval callback for resume
                Some(progress_cb),
                "telegram",
                Some(&chat_id_str),
            )
            .await
    } else {
        agent
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
            .await
    };

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
    // #1377: if a flow card survived to settle, register its state handle so
    // later background-task completions fold their acks into THIS card
    // instead of spraying standalone bubbles. Overwritten by the next
    // settle; cleared on teardown (delivery.rs).
    if streaming
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .open_group_msg_id
        .is_some()
    {
        telegram_state
            .register_flow_state(session_id, Arc::clone(&streaming))
            .await;
    }
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
        // #31: the sign-off trailer rides to render_suggestions with the
        // merge host — same last-out-of-the-flow ordering as handle_message.
        let (merge_host, trailer) = {
            let mut s = streaming.lock().unwrap_or_else(|e| e.into_inner());
            (s.final_bubble.take(), s.pending_trailer.take())
        };
        super::suggest_options::render_suggestions(
            &bot,
            &telegram_state,
            session_id,
            chat_id,
            thread_id,
            options,
            merge_host,
            trailer,
        )
        .await;
    }

    Ok(())
}

/// Cap for the #1221 echo body: classic `sendMessage` caps a message at 4096
/// chars; header, tags and Telegram's own margin eat the rest of the budget.
const BG_ECHO_BODY_CAP_CHARS: usize = 3200;

/// Producer stamped into a `[session-notify from=…]` header.
///
/// Cross-session pushes name the real sender session uuid (the agent tool,
/// #1203) — the echo humanizes it via [`sender_label`]. The CLI lane (#23)
/// has NO sender session (the verb runs as a separate process), so it
/// stamps `cli:<label>` ([`CLI_SENDER_PREFIX`]); the echo renders the
/// carried label verbatim instead of a session lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotifySender<'a> {
    /// A real sender session (`from=<uuid>`).
    Session(Uuid),
    /// The CLI lane (`from=cli:<label>`, #23): the label renders verbatim.
    CliTooling(&'a str),
}

/// Split the mechanical `[session-notify from=<uuid>]` header (#1203) off a
/// push's context text. Absent or malformed header → `(None, whole text)`.
/// A `cli:`-prefixed sender (#23) yields [`NotifySender::CliTooling`]
/// carrying the label after the prefix; an EMPTY label is malformed and
/// falls through to `(None, whole text)`.
pub(crate) fn split_notify_header(context_text: &str) -> (Option<NotifySender<'_>>, &str) {
    let trimmed = context_text.trim_start();
    let Some(after_open) = trimmed.strip_prefix("[session-notify from=") else {
        return (None, trimmed);
    };
    let Some(close) = after_open.find(']') else {
        return (None, trimmed);
    };
    let rest = after_open[close + 1..].trim_start();
    let candidate = &after_open[..close];
    if let Some(label) = candidate.strip_prefix(CLI_SENDER_PREFIX) {
        let label = label.trim();
        if !label.is_empty() {
            return (Some(NotifySender::CliTooling(label)), rest);
        }
    }
    match Uuid::parse_str(candidate) {
        Ok(sender) => (Some(NotifySender::Session(sender)), rest),
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
pub(crate) fn split_bg_echo_parts(context_text: &str) -> (Option<NotifySender<'_>>, String) {
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

/// The bg-receipt tail fence (#15): three backticks normally, but ONE MORE
/// than the longest backtick run when the tail itself carries a ``` run, so
/// raw output can never escape the code block and corrupt the card.
fn receipt_fence(tail: &str) -> String {
    let (mut longest, mut run) = (0usize, 0usize);
    for ch in tail.chars() {
        if ch == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    "`".repeat(if longest >= 3 { longest + 1 } else { 3 })
}

/// Pure line builder for the folded ack (#1377): same shapes as the receipt
/// card summary — backtick-stripped label (empty → "background task"),
/// humanized duration, first-line tail preview. Empty tail drops the
/// preview suffix entirely.
pub(crate) fn bg_ack_line(meta: &crate::brain::agent::BgTaskMeta) -> String {
    let icon = if meta.success { "✅" } else { "❌" };
    let stripped = meta.label.replace('`', "");
    let label = stripped.trim();
    let label = if label.is_empty() {
        "background task"
    } else {
        label
    };
    let duration = background_tasks::format_elapsed(meta.elapsed_secs);
    let preview = first_line_preview(&meta.tail);
    if preview.is_empty() {
        format!("{icon} `{label}` 🕒 {duration}")
    } else {
        format!("{icon} `{label}` 🕒 {duration} · {preview}")
    }
}

/// Pure state mutation for the fold (#1377): append the ack line as system
/// chrome (never reclaimed as the turn's answer, #1253) and re-stamp the
/// header counters. Refuses (returns false, touches nothing) when the state
/// has no card — the caller then falls back to the standalone bubble lane.
pub(crate) fn apply_bg_ack_fold(
    s: &mut crate::channels::telegram::flow::StreamingState,
    line: String,
    bg_indicator: Option<String>,
    bg_count: Option<usize>,
) -> bool {
    if s.open_group_msg_id.is_none() {
        return false;
    }
    s.flow_entries
        .push(crate::channels::telegram::flow::FlowEntry::System(line));
    s.bg_indicator = bg_indicator;
    s.bg_count = bg_count;
    true
}

/// Fold a bare background-task ack into the session's settled flow card
/// (#1377). Appends ONE system line (`✅ \`label\` 🕒 duration · preview`) to
/// the collapsible block, re-stamps the header counters from the live
/// registry (so "⏳ Waiting for N background tasks" decrements and flips to
/// "✅ Finished" at zero), and re-renders the card in place via the normal
/// governor path (Status class: droppable under flood, self-healing next
/// tick). Returns false — caller falls back to the standalone bubble — when
/// the session has no registered settled card or the card was torn down.
pub(crate) async fn fold_bg_ack_into_flow_card(
    bot: &Bot,
    chat_id: i64,
    state: &Arc<TelegramState>,
    agent: &Arc<AgentService>,
    session_id: Uuid,
    meta: &crate::brain::agent::BgTaskMeta,
) -> bool {
    let Some(streaming) = state.flow_state_for(session_id).await else {
        return false;
    };
    let line = bg_ack_line(meta);
    let (bg_indicator, bg_count) = super::delivery::bg_indicator_for(agent, session_id);
    let folded = {
        let mut s = streaming.lock().unwrap_or_else(|e| e.into_inner());
        apply_bg_ack_fold(&mut s, line, bg_indicator, bg_count)
    };
    if !folded {
        return false;
    }
    super::flow::refresh_flow(
        bot,
        teloxide::types::ChatId(chat_id),
        &streaming,
        super::governor::EditClass::Status,
    )
    .await;
    true
}

/// Background-task receipt card (#15, owner-locked shape P3f): ONE collapsed
/// `<details>`. Summary = `<sub>{✅|❌} `{label}` 🕒 {duration}</sub>` — icon
/// by exit 0 / non-zero as the sole outcome signal (no exit code, no
/// wording), label = the roster `short_label` form in inline code. Body =
/// ONE fenced code block with the output tail verbatim. Returns (rich
/// markdown, classic HTML fallback). The markdown leg feeds
/// [`super::send::send_markdown_outbox`], whose native-rich route renders
/// the collapsible + code block server-side — the shape the owner-approved
/// prototypes proved on screen (topic 31847).
pub(crate) fn build_bg_receipt_card(meta: &crate::brain::agent::BgTaskMeta) -> (String, String) {
    let icon = if meta.success { "✅" } else { "❌" };
    // The label sits inside an inline-code span: backticks would escape the
    // span and break the summary, so they are stripped.
    let stripped = meta.label.replace('`', "");
    let label = stripped.trim();
    let label = if label.is_empty() {
        "background task"
    } else {
        label
    };
    let duration = background_tasks::format_elapsed(meta.elapsed_secs);
    let fence = receipt_fence(&meta.tail);
    let markdown = format!(
        "<details>\n<summary><sub>{icon} `{label}` 🕒 {duration}</sub></summary>\n\n\
         {fence}\n{tail}\n{fence}\n\n</details>",
        tail = meta.tail
    );
    // Degraded path: same content as a classic blockquote (non-collapsible
    // on this wire), computed up front and only touched if the outbox send
    // fails (#1234 fallback discipline).
    let flat_title = format!("{icon} {label} 🕒 {duration}");
    let fenced_body = format!("{fence}\n{tail}\n{fence}", tail = meta.tail);
    let (_, classic_html) = build_bg_echo_bubble(&fenced_body, &flat_title);
    (markdown, classic_html)
}

/// The notify-card peek (#15 amendment): the body's first line, truncated
/// ~45 chars + ellipsis. Deterministic at compose time — no new metadata.
fn first_line_preview(body: &str) -> String {
    let line = body.lines().next().unwrap_or("").trim();
    if line.chars().count() > 45 {
        format!("{}…", crate::utils::string::truncate_chars(line, 45))
    } else {
        line.to_string()
    }
}

/// Session-notify receipt card (#15 amendment, owner-locked shape N4): ONE
/// collapsed `<details>`. Summary = `<sub>📨 From <b>{sender}</b>: {preview}</sub>`
/// — fixed 📨 (notifies carry no success/failure semantics), sender label in
/// bold, preview = truncated first line of the body. Body = the notify
/// content as rendered markdown: prose and pipe tables stay native for the
/// rich parser — no code fence, a notify is a document, not a log.
pub(crate) fn build_notify_receipt_card(sender_label: &str, body: &str) -> (String, String) {
    // The sender sits inside a <b> tag: angle brackets are neutralized so a
    // label containing '<' cannot open a tag and corrupt the summary.
    let sanitized = sender_label.replace('<', "‹").replace('>', "›");
    let sender = sanitized.trim();
    let sender = if sender.is_empty() {
        "session notify"
    } else {
        sender
    };
    let preview = first_line_preview(body);
    let truncated = body.chars().count() > BG_ECHO_BODY_CAP_CHARS;
    let body = crate::utils::string::truncate_chars(body, BG_ECHO_BODY_CAP_CHARS);
    let suffix = if truncated { " (truncated)" } else { "" };
    let markdown = format!(
        "<details>\n<summary><sub>📨 From <b>{sender}</b>: {preview}</sub></summary>\n\n\
         {body}{suffix}\n\n</details>"
    );
    let flat_title = format!("📨 From {sender}: {preview}");
    let (_, classic_html) = build_bg_echo_bubble(&format!("{body}{suffix}"), &flat_title);
    (markdown, classic_html)
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
/// sender chat == recipient chat → the sender's topic name; a DM session
/// (positive chat id) → the BOT's username from the startup get_me cache
/// (the reader IS the chat's human side — handing their own name back as
/// the sender is useless); a different non-forum chat → chat name; a
/// different forum → chat + topic name. The card summary renders it as
/// `From <name>:`. Topic names come from the local
/// `channel_messages.topic_name` store (the Bot API has no method to query
/// them), chat names from `getChat`. Best-effort: any lookup failure falls
/// back to the short session id (the same prefix `session_search list`
/// displays).
pub(crate) async fn sender_label(
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
            Some(tid) => local_topic_name(sc, tid).await,
            None => None,
        },
        // DM with the bot (positive chat id = private chat): label the bot
        // itself from the startup get_me cache — zero network. An empty
        // cache (shouldn't happen post-boot) falls through to the short-id
        // fallback below.
        Some(sc) if sc > 0 => state.bot_username().await,
        Some(sc) => {
            let chat =
                super::titles::chat_title(&api_url, &token, teloxide::types::ChatId(sc)).await;
            match (chat, sender_topic) {
                (Some(c), Some(tid)) => match local_topic_name(sc, tid).await {
                    Some(t) => Some(format!("{c} / {t}")),
                    None => Some(c),
                },
                (Some(c), None) => Some(c),
                (None, _) => None,
            }
        }
    };
    label.unwrap_or_else(|| short_session_id(sender))
}

/// Forum-topic display name, resolved LOCALLY: the Bot API has no method to
/// query a topic's title (`getForumTopic` does not exist — bots can only
/// learn names passively; telegram-bot-api issues #634/#356). The handler
/// already stores every observed topic name in `channel_messages.topic_name`,
/// so the card builder reads the newest one for (chat, thread). Any miss —
/// DB unavailable, topic never observed, empty name — feeds the short-id
/// fallback in `sender_label` like every other lookup failure.
async fn local_topic_name(chat_id: i64, thread_id: i32) -> Option<String> {
    let pool = crate::db::global_pool()?;
    let repo = ChannelMessageRepository::new(pool.clone());
    repo.latest_topic_name("telegram", &chat_id.to_string(), &thread_id.to_string())
        .await
        .ok()
        .flatten()
}

/// Short session id: first 8 hex chars of the uuid (matches `session_search`
/// list's short-id prefix display).
pub(crate) fn short_session_id(uuid: Uuid) -> String {
    uuid.simple().to_string()[..8].to_owned()
}

/// How recently (seconds) a Telegram-bound session must have been active to
/// appear in the boot-time wake log (#1227).
///
/// Kept short on purpose: a back-to-back dev restart (the recurring case on
/// the ops box, 10+/day) must not flood the log with every recently-active
/// session each time. Only sessions that touched their topic inside this
/// window are recorded.
pub const WAKE_RECENT_SECS: i64 = 600;

/// Log every Telegram-bound session that was active shortly before boot but
/// is not currently being resumed, so the stranded set is auditable (#1227).
///
/// The on-disk journal only rescues turns that were literally mid-loop at the
/// kill instant; between-turn sessions have no row (#1224 re-registers their
/// delivery routes, which drains parked reports, but that happens quietly).
/// This pass names them in the log instead: it runs no turn, re-executes
/// nothing, and sends no message — the former "I'm back" bubble was removed
/// per #34 (owner direction 2026-08-29: remove the UX message, leave
/// logging), because it promised resumption the feature does not yet deliver
/// and was never persisted in `channel_messages`, so it could not be audited.
///
/// Sessions whose ids are in `already_resumed` are skipped: those were roused
/// by a full continuation prompt via `resume_session` already. Returns how
/// many sessions landed in the log.
pub async fn wake_recently_active(
    pool: crate::db::Pool,
    already_resumed: &std::collections::HashSet<Uuid>,
) -> usize {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let since_epoch = now.saturating_sub(WAKE_RECENT_SECS);
    let repo = crate::db::SessionBindingRepository::new(pool);
    let Ok(bindings) = repo.recent_for_channel("telegram", since_epoch).await else {
        tracing::warn!(target: "telegram", "Boot wake could not read recent session bindings");
        return 0;
    };

    let mut stranded: Vec<String> = Vec::new();
    for b in bindings {
        let Ok(sid) = Uuid::parse_str(&b.session_id) else {
            continue;
        };
        if already_resumed.contains(&sid) {
            continue;
        }
        stranded.push(short_session_id(sid));
    }

    let scheduled = stranded.len();
    if scheduled > 0 {
        tracing::info!(
            target: "telegram",
            "Boot wake pass (log-only, #34): recently-active sessions not resumed: [{}]",
            stranded.join(",")
        );
        tracing::info!(
            target: "telegram",
            "Scheduled boot wake for {scheduled} recently-active session(s) (#1227)"
        );
    }
    scheduled
}
