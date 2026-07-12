//! Crash-recovery resume: replays an interrupted Telegram turn on startup
//! with full streaming (typing, tool messages, edit loop, final delivery).
//!
//! Moved VERBATIM out of handler.rs (#471 phase 1, pure decomposition —
//! the handler glob re-export keeps every existing call site stable).

use super::TelegramState;
#[allow(unused_imports)]
use super::handler::*;
use super::send::{chat_action_in_thread, message_in_thread};
use crate::brain::agent::{AgentService, ProgressCallback, ProgressEvent};
use crate::config::Config;
use crate::db::ChannelMessageRepository;
use crate::utils::sanitize::redact_secrets;
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::ChatAction;
use teloxide::types::{MessageId, ParseMode};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

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
    tokio::spawn({
        let bot = bot.clone();
        let cancel = typing_cancel.clone();
        async move {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(std::time::Duration::from_secs(4)) => {
                        let _ = chat_action_in_thread(&bot, chat_id, thread_id,  ChatAction::Typing).await;
                    }
                }
            }
        }
    });

    // ── Streaming setup ────────────────────────────────────────────────────
    let streaming = Arc::new(std::sync::Mutex::new(StreamingState {
        msg_id: None,
        thinking: String::new(),
        tool_msgs: Vec::new(),
        display_queue: Vec::new(),
        open_group_msg_id: None,
        flow_entries: Vec::new(),
        flow_status: None,
        flow_rich: false,
        response: String::new(),
        dirty: false,
        recreate: false,
        header_preview: None,
        sections: Default::default(),
        applied_plan_kb: Default::default(),
        tool_round_count: 0,
        tools_started_at: Some(std::time::Instant::now()),
        turn_started_at: std::time::Instant::now(),
        flow_outcome: None,
        sent_intermediates: Vec::new(),
        intermediate_msg_ids: Vec::new(),
        voice_msg_ids: Vec::new(),
        processing: true,
        // resume_session restarts an interrupted turn; the user did
        // not just type a fresh message, so there's no preview to
        // surface in the rolling status line. The status path in
        // resume_session also doesn't currently emit rolling
        // messages — left as None for forward compatibility.
        user_message_preview: None,
        header_query_shown: false,
    }));

    let edit_cancel = CancellationToken::new();

    // Edit loop — same as handle_message
    // Store JoinHandle to await after cancellation (prevents duplicate race).
    let edit_loop_handle = tokio::spawn({
        let bot = bot.clone();
        let st = streaming.clone();
        let cancel = edit_cancel.clone();
        let tg = telegram_state.clone();
        let agent = agent.clone();
        let sid = session_id;
        async move {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(std::time::Duration::from_millis(1500)) => {
                        struct Snap {
                            dirty: bool,
                            recreate: bool,
                            response_text: String,
                            msg_id: Option<MessageId>,
                            display_items: Vec<DisplayItem>,
                            /// Any tool flipped status this tick (needs a flow re-render).
                            tools_dirty: bool,
                            has_active_tools: bool,
                            tool_round_count: usize,
                            processing: bool,
                            thinking_excerpt: Option<String>,
                        }

                        let snap = {
                            let mut s = st.lock().unwrap_or_else(|e| e.into_inner());
                            let has_display = !s.display_queue.is_empty();
                            let any_tools_dirty = s.tool_msgs.iter().any(|t| t.dirty);
                            let has_active_tools =
                                s.tool_msgs.iter().any(|t| t.completed.is_none());
                            let processing = s.processing;
                            // Same wake conditions as the main handler loop, so a
                            // resumed turn ticks its live header while tools run
                            // instead of only waking on new display items.
                            if !s.dirty
                                && !s.recreate
                                && !has_display
                                && !any_tools_dirty
                                && !has_active_tools
                                && !processing
                            {
                                continue;
                            }
                            let items: Vec<DisplayItem> = s.display_queue.drain(..).collect();
                            for t in s.tool_msgs.iter_mut().filter(|t| t.dirty) {
                                t.dirty = false;
                            }
                            let response_text = s.render();
                            let snap = Snap {
                                dirty: s.dirty,
                                recreate: s.recreate,
                                response_text,
                                msg_id: s.msg_id,
                                display_items: items,
                                tools_dirty: any_tools_dirty,
                                has_active_tools,
                                tool_round_count: s.tool_round_count,
                                processing,
                                thinking_excerpt: thinking_status_excerpt(&s.thinking),
                            };
                            s.dirty = false;
                            s.recreate = false;
                            snap
                        };

                        // A new round landed this tick iff there were display
                        // items to fold in. Saved before the loop consumes them,
                        // for the buried-block re-stick check below (#451).
                        let had_round = !snap.display_items.is_empty();

                        // Process display items (tools + intermediates)
                        // Buffer consecutive tool calls to group them into collapsible blocks
                        let mut tool_buffer: Vec<usize> = Vec::new();

                        for item in snap.display_items {
                            match item {
                                DisplayItem::NewTool(idx) => {
                                    tool_buffer.push(idx);
                                }
                                DisplayItem::Intermediate(text) => {
                                    // Fold the intermediate into the open
                                    // processing-log flow (#300). A resumed
                                    // session has no inbound user message, so a
                                    // <<react:>> directive is stripped but no
                                    // reaction fires (#261).
                                    append_tool_group(&bot, chat_id, thread_id, &st, &tool_buffer)
                                        .await;
                                    tool_buffer.clear();
                                    let text = crate::utils::sanitize::strip_llm_artifacts(&text);
                                    let text = redact_secrets(&text);
                                    let (text, _img_paths) =
                                        crate::utils::extract_img_markers(&text);
                                    let (text, _react_emoji) =
                                        crate::utils::extract_react_marker(&text);
                                    append_intermediate_to_flow(
                                        &bot, chat_id, thread_id, &st, &text,
                                    )
                                    .await;
                                }
                            }
                        }

                        // Flush any remaining tools into the open group (kept open
                        // so the next tick's tools append to this same message).
                        append_tool_group(&bot, chat_id, thread_id, &st, &tool_buffer).await;

                        // Re-stick the open block to the bottom if newer chatter
                        // buried it (#451), only on a real round.
                        if had_round {
                            let newest = tg.newest_incoming_msg_id(chat_id.0);
                            restick_flow_if_buried(&bot, chat_id, thread_id, &st, newest).await;
                        }

                        // ── Live flow header parity with the main handler ──
                        // Resume rides the same shared tick: open the flow
                        // early (header-only), roll the wall-clock duration,
                        // thinking preview, and plan/goal/ctx sections. A
                        // resumed turn has no fresh user message, so there is
                        // no Working-on fallback.
                        let show_status = snap.has_active_tools
                            || (snap.tool_round_count > 0 && snap.response_text.is_empty())
                            || snap.processing;
                        let turn_done = snap.dirty && !snap.response_text.is_empty();
                        let preview = snap
                            .thinking_excerpt
                            .as_deref()
                            .map(|t| format!("🧠 {t}"));
                        super::flow_chrome::tick_flow_header(
                            &bot,
                            chat_id,
                            thread_id,
                            &st,
                            &agent,
                            sid,
                            show_status,
                            turn_done,
                            preview,
                            snap.tools_dirty,
                        )
                        .await;

                        // Response message (streaming). Stale-placeholder cleanup
                        // runs unconditionally so a bubble opened before the first
                        // tool call is still removed once a block opens.
                        if snap.recreate
                            && let Some(old_mid) = snap.msg_id
                        {
                            let _ = bot.delete_message(chat_id, old_mid).await;
                            let mut s = st.lock().unwrap_or_else(|e| e.into_inner());
                            s.msg_id = None;
                        }
                        // While a processing-log block is open, mid-turn narration
                        // folds into it and the final answer is delivered by
                        // deliver_final_response at turn end. Opening a standalone
                        // streaming bubble here leaks the intermediate text as its
                        // own message beneath the folded block (#490). handle_message
                        // gained this guard; resume was missing it, so a resumed
                        // turn with an open block still leaked the bubble.
                        let open_block = {
                            let s = st.lock().unwrap_or_else(|e| e.into_inner());
                            s.open_group_msg_id
                        };
                        if (snap.dirty || snap.recreate)
                            && open_block.is_none()
                            && !snap.response_text.is_empty()
                        {
                            let current_msg_id = {
                                let s = st.lock().unwrap_or_else(|e| e.into_inner());
                                s.msg_id
                            };
                            if current_msg_id.is_none()
                                && let Ok(m) = message_in_thread(&bot, chat_id, thread_id,  "\u{258b}").await
                            {
                                let mut s = st.lock().unwrap_or_else(|e| e.into_inner());
                                s.msg_id = Some(m.id);
                            }
                            let msg_id = {
                                let s = st.lock().unwrap_or_else(|e| e.into_inner());
                                s.msg_id
                            };
                            if let Some(mid) = msg_id {
                                // Strip any complete <<react:emoji>> directive from
                                // the streaming snapshot so the raw marker never
                                // flashes in the placeholder (#261). Reaction fires
                                // from the intermediate/final paths.
                                let (clean, _) =
                                    crate::utils::extract_react_marker(&snap.response_text);
                                let html = markdown_to_telegram_html(&clean);
                                let display = format!("{}\u{258b}", html);
                                let _ = bot
                                    .edit_message_text(chat_id, mid, display)
                                    .parse_mode(ParseMode::Html)
                                    .await;
                            }
                        }

                        let _ = chat_action_in_thread(&bot, chat_id, thread_id,  ChatAction::Typing).await;
                    }
                }
            }
        }
    });

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
                    let _ = chat_action_in_thread(&bot, chat, thread_id, ChatAction::Typing).await;
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
            _ => {}
        })
    };

    // ── Agent call ──────────────────────────────────────────────────────────
    let cancel_token = CancellationToken::new();
    telegram_state
        .store_cancel_token(session_id, cancel_token.clone())
        .await;

    let chat_id_str = chat_id.0.to_string();
    let question_cb = super::follow_up_question::make_question_callback(
        telegram_state.clone(),
        streaming.clone(),
    );
    let result = agent
        .send_message_with_tools_and_callback(
            session_id,
            prompt,
            None,
            Some(cancel_token.clone()),
            None, // no approval callback for resume
            Some(progress_cb),
            Some(question_cb),
            "telegram",
            Some(&chat_id_str),
        )
        .await;

    telegram_state.remove_cancel_token(session_id).await;
    edit_cancel.cancel();
    // Await edit loop to prevent race where it sends a NEW message after
    // we grab streaming_msg_id (causes duplicate completion).
    let _ = edit_loop_handle.await;

    // ── Final delivery ─────────────────────────────────────────────────────
    let (streaming_msg_id, remaining_display) = {
        let mut s = streaming.lock().unwrap_or_else(|e| e.into_inner());
        let display: Vec<DisplayItem> = s.display_queue.drain(..).collect();
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
            let _ = bot.delete_message(chat_id, mid).await;
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
    }
    refresh_flow(&bot, chat_id, &streaming).await;

    Ok(())
}
