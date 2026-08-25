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
                state.enqueue_reaction(session_id, msg);
                return;
            };

            let thread_id = super::send::latest_thread_id_for_chat(chat_id).await;
            if let Err(e) = resume_session(
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

/// Resume an interrupted session with full streaming (typing, tool messages, edit loop).
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
    refresh_flow(&bot, chat_id, &streaming).await;
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
