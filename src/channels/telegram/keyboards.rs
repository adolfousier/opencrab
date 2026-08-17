//! Inline-keyboard builders and the tool-approval callback: the approval
//! prompt with Yes/Always/No buttons, the /cd directory browser keyboard
//! and the profile picker keyboard.
//!
//! Moved VERBATIM out of handler.rs (#471 phase 1, pure decomposition —
//! the handler glob re-export keeps every existing call site and test
//! import stable).

use super::markdown::escape_html;
use std::sync::Arc;
use teloxide::types::InlineKeyboardButton;

/// Build an `ApprovalCallback` that sends an inline-keyboard message to Telegram
/// and waits (up to 5 min) for the user to tap Yes, Always, or No.
pub(crate) fn make_approval_callback(
    state: Arc<super::TelegramState>,
) -> crate::brain::agent::ApprovalCallback {
    use crate::brain::agent::ToolApprovalInfo;
    use crate::utils::{check_approval_policy, persist_auto_session_policy};
    use teloxide::payloads::SendMessageSetters;
    use teloxide::types::{ChatId, InlineKeyboardButton, InlineKeyboardMarkup, ParseMode};
    use tokio::sync::oneshot;

    Arc::new(move |info: ToolApprovalInfo| {
        let state = state.clone();
        Box::pin(async move {
            // Respect config-level approval policy (single source of truth)
            if let Some(result) = check_approval_policy() {
                return Ok(result);
            }

            // Find the chat this session is active in
            let chat_id = match state.session_chat(info.session_id).await {
                Some(id) => id,
                None => match state.owner_chat_id().await {
                    Some(id) => id,
                    None => {
                        tracing::warn!(
                            "Telegram approval: no chat_id for session {}",
                            info.session_id
                        );
                        return Ok((false, false));
                    }
                },
            };

            let bot = match state.bot().await {
                Some(b) => b,
                None => {
                    tracing::warn!("Telegram approval: bot not connected");
                    return Ok((false, false));
                }
            };

            // Build unique approval id
            let approval_id = uuid::Uuid::new_v4().to_string();

            // Build inline keyboard — Yes / Always (session) / YOLO (permanent) / No
            let keyboard = InlineKeyboardMarkup::new(vec![
                vec![
                    InlineKeyboardButton::callback("✅ Yes", format!("approve:{}", approval_id)),
                    InlineKeyboardButton::callback(
                        "🔁 Always (session)",
                        format!("always:{}", approval_id),
                    ),
                ],
                vec![
                    InlineKeyboardButton::callback(
                        "🔥 YOLO (permanent)",
                        format!("yolo:{}", approval_id),
                    ),
                    InlineKeyboardButton::callback("❌ No", format!("deny:{}", approval_id)),
                ],
            ]);

            // Format message — redact secrets before display, truncate to fit Telegram limit
            let safe_input = crate::utils::redact_tool_input(&info.tool_input);
            let mut input_pretty = serde_json::to_string_pretty(&safe_input)
                .unwrap_or_else(|_| safe_input.to_string());
            if input_pretty.len() > 3500 {
                input_pretty.truncate(3500);
                input_pretty.push_str("\n... [truncated]");
            }
            let text = format!(
                "🔐 <b>Tool Approval Required</b>\n\nTool: <code>{}</code>\nInput:\n<pre>{}</pre>",
                info.tool_name,
                escape_html(&input_pretty),
            );

            // Register oneshot channel BEFORE sending the message to prevent
            // race condition where user clicks before registration completes
            let (tx, rx) = oneshot::channel();
            state
                .register_pending_approval(approval_id.clone(), tx)
                .await;
            tracing::info!(
                "Telegram approval: registered pending id={}, sending to chat={}",
                approval_id,
                chat_id
            );

            // Resolve forum topic_id for this session (#249)
            let topic_id = state
                .session_topic(info.session_id)
                .await
                .map(|tid| teloxide::types::ThreadId(teloxide::types::MessageId(tid)));

            match super::send::message_in_thread(&bot, ChatId(chat_id), topic_id, &text)
                .parse_mode(ParseMode::Html)
                .reply_markup(keyboard)
                .await
            {
                Ok(_) => {
                    tracing::info!(
                        "Telegram approval: message sent, waiting for response (id={})",
                        approval_id
                    );
                }
                Err(e) => {
                    tracing::error!("Telegram approval: failed to send message: {}", e);
                    return Ok((false, false));
                }
            }

            // Wait up to 5 minutes
            match tokio::time::timeout(std::time::Duration::from_secs(300), rx).await {
                Ok(Ok((approved, always))) => {
                    tracing::info!(
                        "Telegram approval: user responded id={}, approved={}, always={}",
                        approval_id,
                        approved,
                        always
                    );
                    if always {
                        persist_auto_session_policy();
                    }
                    Ok((approved, always))
                }
                Ok(Err(_)) => {
                    tracing::warn!(
                        "Telegram approval: oneshot channel closed (id={})",
                        approval_id
                    );
                    Ok((false, false))
                }
                Err(_) => {
                    tracing::warn!(
                        "Telegram approval: 5-minute timeout — auto-denying (id={})",
                        approval_id
                    );
                    Ok((false, false))
                }
            }
        })
    })
}

/// Build inline keyboard rows for the /cd directory browser.
///
/// Layout:
/// - One row per entry (dir with 📁, file with 📄)
/// - [⬆️ Parent] row if not at root
/// - [◀️ Prev] [Page N/M] [Next ▶️] pagination row (if >1 page)
/// - [✅ Select this directory] confirm row
pub(crate) fn build_cd_keyboard(
    resp: &crate::channels::commands::DirBrowserResponse,
) -> Vec<Vec<InlineKeyboardButton>> {
    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();

    // Entry buttons — each entry gets its own row for readability
    for entry in &resp.entries {
        let icon = if entry.is_dir { "📁" } else { "📄" };
        let display = format!("{} {}", icon, entry.name);
        rows.push(vec![InlineKeyboardButton::callback(
            display,
            format!("cd:sel:{}", entry.index),
        )]);
    }

    // Parent directory button (unless at filesystem root)
    let is_root = resp.current_path == "/" || resp.current_path.len() <= 1;
    if !is_root {
        rows.push(vec![InlineKeyboardButton::callback(
            "⬆️ Parent",
            "cd:up".to_string(),
        )]);
    }

    // Pagination row (only if >1 page)
    if resp.total_pages > 1 {
        let mut pag_row = Vec::new();
        if resp.page > 0 {
            pag_row.push(InlineKeyboardButton::callback(
                "◀️ Prev",
                format!("cd:pg:{}", resp.page - 1),
            ));
        }
        pag_row.push(InlineKeyboardButton::callback(
            format!("📄 {}/{}", resp.page + 1, resp.total_pages),
            "cd:noop".to_string(),
        ));
        if resp.page + 1 < resp.total_pages {
            pag_row.push(InlineKeyboardButton::callback(
                "Next ▶️",
                format!("cd:pg:{}", resp.page + 1),
            ));
        }
        rows.push(pag_row);
    }

    // Confirm button
    rows.push(vec![InlineKeyboardButton::callback(
        "✅ Select this directory",
        "cd:here".to_string(),
    )]);

    rows
}

pub(crate) fn build_profiles_keyboard(
    resp: &crate::channels::commands::ProfilesResponse,
) -> Vec<Vec<InlineKeyboardButton>> {
    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();

    // Each profile gets its own row
    for entry in &resp.entries {
        let icon = if entry.is_active { "▸" } else { "•" };
        let active_tag = if entry.is_active { " ✓" } else { "" };
        let display = format!("{} {}{}", icon, entry.name, active_tag);
        rows.push(vec![InlineKeyboardButton::callback(
            display,
            format!("prof:sel:{}", entry.name),
        )]);
    }

    // Action row: create new profile
    rows.push(vec![InlineKeyboardButton::callback(
        "➕ New Profile",
        "prof:create".to_string(),
    )]);

    rows
}

/// Best-effort callback ack: clears the button spinner for `query`.
///
/// Failures are logged (not propagated): an ack racing a deleted message or
/// a slow Telegram round-trip must never break the surrounding handler —
/// the action itself has already run by the time the ack fires. Same shape
/// as `fire_reaction` / `best_effort_delete`.
pub(crate) async fn ack_callback(
    bot: &teloxide::Bot,
    query: &teloxide::types::CallbackQuery,
    why: &str,
) {
    use teloxide::prelude::Requester;
    if let Err(e) = bot.answer_callback_query(query.id.clone()).await {
        tracing::warn!(
            target: "telegram::send",
            why,
            query_id = %query.id,
            error = %e,
            "callback ack failed"
        );
    }
}
