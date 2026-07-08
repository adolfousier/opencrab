//! Crash-recovery resume: replays an interrupted Telegram turn on startup
//! with full streaming (typing, tool messages, edit loop, final delivery).
//!
//! Moved VERBATIM out of handler.rs (#471 phase 1, pure decomposition —
//! the handler glob re-export keeps every existing call site stable).

use super::TelegramState;
#[allow(unused_imports)]
use super::handler::*;
use super::send::{chat_action_in_thread, message_in_thread, photo_in_thread};
use crate::brain::agent::{AgentService, ProgressCallback, ProgressEvent};
use crate::utils::sanitize::redact_secrets;
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::ChatAction;
use teloxide::types::{InputFile, MessageId, ParseMode};
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
        status_msg_id: None,
        status_last_text: None,
        tool_round_count: 0,
        tools_started_at: Some(std::time::Instant::now()),
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
    }));

    let edit_cancel = CancellationToken::new();

    // Edit loop — same as handle_message
    // Store JoinHandle to await after cancellation (prevents duplicate race).
    let edit_loop_handle = tokio::spawn({
        let bot = bot.clone();
        let st = streaming.clone();
        let cancel = edit_cancel.clone();
        let tg = telegram_state.clone();
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
                        }

                        let snap = {
                            let mut s = st.lock().unwrap_or_else(|e| e.into_inner());
                            let has_display = !s.display_queue.is_empty();
                            if !s.dirty && !s.recreate && !has_display { continue; }
                            let items: Vec<DisplayItem> = s.display_queue.drain(..).collect();
                            let response_text = s.render();
                            let snap = Snap {
                                dirty: s.dirty,
                                recreate: s.recreate,
                                response_text,
                                msg_id: s.msg_id,
                                display_items: items,
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

                        // Response message (streaming)
                        if snap.dirty || snap.recreate {
                            if snap.recreate
                                && let Some(old_mid) = snap.msg_id
                            {
                                let _ = bot.delete_message(chat_id, old_mid).await;
                                let mut s = st.lock().unwrap_or_else(|e| e.into_inner());
                                s.msg_id = None;
                            }
                            if !snap.response_text.is_empty() {
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
                                    // Strip any complete <<react:emoji>>
                                    // directive from the streaming snapshot so
                                    // the raw marker never flashes in the
                                    // placeholder (#261). Reaction fires from
                                    // the intermediate/final paths.
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
                    let idx = s.tool_msgs.len();
                    s.tool_msgs.push(ToolMsg {
                        msg_id: None,
                        name: tool_name,
                        context: ctx,
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
    let (mut streaming_msg_id, remaining_display) = {
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

    match result {
        Ok(response) => {
            let (text_only, img_paths) = crate::utils::extract_img_markers(&response.content);
            let text_only = crate::utils::sanitize::strip_llm_artifacts(&text_only);
            let text_only = redact_secrets(&text_only);

            // Extract <<react:emoji>> directive — see handle_message.
            let (text_only, react_emoji) = crate::utils::extract_react_marker(&text_only);

            // Dedup intermediates already delivered so we don't duplicate
            // them when editing the streaming placeholder with the final.
            let sent = {
                let s = streaming.lock().unwrap_or_else(|e| e.into_inner());
                s.sent_intermediates.clone()
            };
            let pre_dedup_text = text_only.clone();
            let text_only = if !sent.is_empty() {
                let mut remaining = text_only.clone();
                for intermediate in &sent {
                    remaining = remaining.replace(intermediate.as_str(), "");
                }
                remaining.trim().to_string()
            } else {
                text_only
            };

            // Reaction-only: if text is empty after dedup and the LLM used
            // <<react:emoji>>, skip delivery. Unlike handle_message, resume
            // has no user message to react to (the original message id is
            // lost across restarts), so we just clean up the placeholder.
            if text_only.trim().is_empty()
                && let Some(ref emoji) = react_emoji
            {
                tracing::info!(
                    "Telegram resume: reaction-only response ({}), skipping delivery",
                    emoji
                );
                if let Some(mid) = streaming_msg_id {
                    let _ = bot.delete_message(chat_id, mid).await;
                }
                return Ok(());
            }

            // Context budget footer is appended to display text, not sent as separate message
            let ctx_max = agent.context_limit_for_session(session_id);
            let footer = crate::utils::format_ctx_footer(
                response.context_tokens,
                ctx_max,
                response.tokens_per_second,
            );

            for img_path in img_paths {
                if let Ok(bytes) = tokio::fs::read(&img_path).await {
                    let _ =
                        photo_in_thread(&bot, chat_id, thread_id, InputFile::memory(bytes)).await;
                }
            }

            // Rich fallback: same logic as handle_message — when all content
            // was sent as HTML intermediates during streaming, replace them
            // with a single native rich message.
            let text_only = if text_only.is_empty()
                && !sent.is_empty()
                && super::rich::should_send_native_rich(&pre_dedup_text)
            {
                let rich_md = if footer.is_empty() {
                    pre_dedup_text.clone()
                } else {
                    format!("{pre_dedup_text}\n\n{footer}")
                };
                match super::rich::api::send_rich_markdown(
                    bot.token(),
                    chat_id.0,
                    thread_id,
                    &rich_md,
                )
                .await
                {
                    Ok(()) => {
                        let intermediate_ids = {
                            let s = streaming.lock().unwrap_or_else(|e| e.into_inner());
                            s.intermediate_msg_ids.clone()
                        };
                        for mid in &intermediate_ids {
                            let _ = bot.delete_message(chat_id, *mid).await;
                        }
                        tracing::info!(
                            "Telegram resume: rich fallback delivered ({} chars), deleted {} HTML intermediates",
                            rich_md.len(),
                            intermediate_ids.len()
                        );
                        text_only
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Telegram resume: rich fallback failed, keeping HTML intermediates: {e}"
                        );
                        text_only
                    }
                }
            } else {
                text_only
            };

            // #300 follow-up: ALWAYS check if the trailing folded text matches the
            // final answer and remove it to prevent duplication (same logic as
            // handle_message above).
            let text_only = if text_only.trim().is_empty() {
                take_folded_final(&bot, chat_id, &streaming)
                    .await
                    .unwrap_or(text_only)
            } else {
                let trailing_matches = {
                    let s = streaming.lock().unwrap_or_else(|e| e.into_inner());
                    match s.flow_entries.last() {
                        Some(FlowEntry::Text(folded)) => {
                            folded_duplicates_final(folded, &text_only)
                        }
                        _ => false,
                    }
                };
                if trailing_matches {
                    take_folded_final(&bot, chat_id, &streaming).await;
                }
                text_only
            };

            let html = markdown_to_telegram_html(&text_only);
            let display_html = if html.is_empty() {
                String::new()
            } else {
                format!("{}\n\n<i>{}</i>", html, footer)
            };
            if !display_html.is_empty() {
                // Rich-first: deliver a structured reply as a fresh native rich
                // message and delete the placeholder on success. resume_session
                // is the path the owner's DM session hits after an interrupted
                // turn, so it must go rich too (handle_message already does), or
                // DMs keep showing the old HTML while groups show rich.
                let delivered_rich = super::rich::should_send_native_rich(&text_only) && {
                    let rich_md = if footer.is_empty() {
                        text_only.clone()
                    } else {
                        format!("{text_only}\n\n<sub>{footer}</sub>")
                    };
                    // Delete the placeholder FIRST so the fresh rich send is the
                    // last message — deleting it after pulls content up and the
                    // view ends mid-chat instead of at the bottom. `.take()`
                    // clears the id so the HTML fallback sends fresh on failure.
                    if let Some(mid) = streaming_msg_id.take() {
                        let _ = bot.delete_message(chat_id, mid).await;
                    }
                    match super::rich::api::send_rich_markdown(
                        bot.token(),
                        chat_id.0,
                        thread_id,
                        &rich_md,
                    )
                    .await
                    {
                        Ok(()) => true,
                        Err(e) => {
                            tracing::warn!(
                                "Telegram resume: rich delivery failed, using HTML: {e}"
                            );
                            false
                        }
                    }
                };

                if !delivered_rich {
                    let chunks: Vec<String> = split_message(&display_html, 4096)
                        .into_iter()
                        .map(|s| s.to_string())
                        .collect();

                    if chunks.len() == 1
                        && let Some(mid) = streaming_msg_id
                    {
                        if let Err(e) = bot
                            .edit_message_text(chat_id, mid, &chunks[0])
                            .parse_mode(ParseMode::Html)
                            .await
                        {
                            tracing::warn!(
                                "Telegram resume: edit failed ({e}), falling back to send"
                            );
                            let _ = bot.delete_message(chat_id, mid).await;
                            let _ = send_html_or_plain(&bot, chat_id, thread_id, &chunks[0]).await;
                        }
                    } else {
                        if let Some(mid) = streaming_msg_id {
                            let _ = bot.delete_message(chat_id, mid).await;
                        }
                        for chunk in &chunks {
                            let _ = send_html_or_plain(&bot, chat_id, thread_id, chunk).await;
                        }
                    }
                }
            } else if let Some(mid) = streaming_msg_id {
                // Empty final text on resume — same as handle_message: append
                // the ctx/tok-s footer to the last intermediate so it isn't
                // dropped, then remove the empty placeholder.
                let last_inter = {
                    let s = streaming.lock().unwrap_or_else(|e| e.into_inner());
                    s.intermediate_msg_ids
                        .last()
                        .copied()
                        .zip(s.sent_intermediates.last().cloned())
                };
                if let Some((inter_id, inter_text)) = last_inter {
                    append_footer_to_last_intermediate(
                        &bot,
                        chat_id,
                        inter_id,
                        &inter_text,
                        &footer,
                    )
                    .await;
                }
                let _ = bot.delete_message(chat_id, mid).await;
            }

            tracing::info!(
                "Telegram: resume completed for session {} — {} chars delivered",
                session_id,
                response.content.len()
            );
        }
        Err(crate::brain::agent::AgentError::Cancelled) => {
            tracing::info!("Telegram: resume cancelled for session {}", session_id);
            if let Some(mid) = streaming_msg_id {
                let _ = bot.delete_message(chat_id, mid).await;
            }
        }
        Err(e) => {
            tracing::error!("Telegram: resume error for session {}: {}", session_id, e);
            if let Some(mid) = streaming_msg_id {
                let _ = bot
                    .edit_message_text(chat_id, mid, format!("Error: {}", e))
                    .await;
            } else {
                let _ = message_in_thread(&bot, chat_id, thread_id, format!("Error: {}", e)).await;
            }
        }
    }

    Ok(())
}
