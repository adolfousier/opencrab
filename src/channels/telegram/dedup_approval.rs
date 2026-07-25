//! Telegram inline-keyboard approval flow for brain dedup proposals (#765).
//!
//! When cross-file brain duplicates are filed (weekly cron, the event-based
//! trigger, or `/dedup`), the dev group can be shown a message listing them
//! with Approve / Reject buttons. Tapping a button resolves here: the
//! proposal is applied (the duplicate text is removed from the target brain
//! file) or rejected, and the message is edited to show the outcome.
//!
//! Upstream scans are strictly report-only. The actual removal only happens
//! on an explicit tap, which keeps the "never auto-apply" contract.
//!
//! Lives in its own module to keep the already-large `agent.rs` focused on
//! the message-routing path (mirrors `follow_up_question.rs`).

use std::path::Path;
use std::sync::Arc;

use teloxide::Bot;
use teloxide::payloads::{EditMessageTextSetters, SendMessageSetters};
use teloxide::prelude::Requester;
use teloxide::types::{
    CallbackQuery, ChatId, InlineKeyboardButton, InlineKeyboardMarkup, ParseMode,
};

use crate::brain::rsi_proposals::{BrainDedupProposal, ProposalsStore};
use crate::brain::tools::dynamic::DynamicToolLoader;
use crate::brain::tools::registry::ToolRegistry;
use crate::brain::tools::rsi_proposals::RsiProposalsTool;

/// Callback-data prefix for every dedup approval button. The dispatcher in
/// `agent.rs` strips this and hands the rest to [`handle_callback`].
pub const DEDUP_PREFIX: &str = "dedup:";

/// Escape the four HTML-special characters teloxide's `ParseMode::Html`
/// recognises. The scanner fields are normally plain path/line references,
/// but escaping defensively keeps a stray `&` or `<` from breaking the send.
fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Build the rsi_proposals tool from a brain dir. Brain dedup apply/reject
/// only touch the proposals store and the brain path, never the tool
/// registry (that is only used when installing dynamic tools/commands/
/// skills), so an empty registry is fine here.
fn build_tool(brain_dir: &Path) -> RsiProposalsTool {
    let registry = Arc::new(ToolRegistry::new());
    let tools_path =
        DynamicToolLoader::default_path().unwrap_or_else(|| brain_dir.join("tools.toml"));
    RsiProposalsTool::new(registry, tools_path, brain_dir.to_path_buf())
}

/// A parsed dedup approval action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DedupAction {
    /// Apply a single proposal by id.
    Apply(String),
    /// Reject a single proposal by id.
    Reject(String),
    /// Apply every pending proposal.
    ApplyAll,
}

/// Parse the callback-data suffix (the part after `dedup:`) into an action.
///
/// Accepted formats: `apply:<id>`, `reject:<id>`, `apply_all`.
pub fn parse_callback(rest: &str) -> Option<DedupAction> {
    if rest == "apply_all" {
        return Some(DedupAction::ApplyAll);
    }
    let (verb, id) = rest.split_once(':')?;
    if id.is_empty() {
        return None;
    }
    match verb {
        "apply" => Some(DedupAction::Apply(id.to_string())),
        "reject" => Some(DedupAction::Reject(id.to_string())),
        _ => None,
    }
}

/// Format the notification message listing pending proposals (HTML).
pub fn format_proposal_message(proposals: &[BrainDedupProposal]) -> String {
    let mut msg = format!(
        "\u{1f9f9} <b>{} brain dedup proposal(s) pending</b>\n\n",
        proposals.len()
    );
    for (i, p) in proposals.iter().enumerate() {
        msg.push_str(&format!(
            "{}. <code>{}</code> {}, duplicates {}\n",
            i + 1,
            escape_html(&p.dedup.target_file),
            escape_html(&p.dedup.line_range),
            escape_html(&p.dedup.duplicate_of),
        ));
    }
    msg.push_str("\nTap \u{2705} to remove a duplicate, \u{274c} to keep it.");
    msg
}

/// Build the approval keyboard: one row per proposal (Approve | Reject),
/// plus a final "Approve all" row when there is more than one proposal.
pub fn build_keyboard(proposals: &[BrainDedupProposal]) -> InlineKeyboardMarkup {
    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    for p in proposals {
        let label = format!("{} {}", p.dedup.target_file, p.dedup.line_range);
        rows.push(vec![
            InlineKeyboardButton::callback(
                format!("\u{2705} {label}"),
                format!("dedup:apply:{}", p.id),
            ),
            InlineKeyboardButton::callback(
                format!("\u{274c} {label}"),
                format!("dedup:reject:{}", p.id),
            ),
        ]);
    }
    if proposals.len() > 1 {
        rows.push(vec![InlineKeyboardButton::callback(
            "\u{2705} Approve all",
            "dedup:apply_all",
        )]);
    }
    InlineKeyboardMarkup::new(rows)
}

/// Send the dedup approval request to a chat. Returns the number of pending
/// proposals shown; `Ok(0)` means nothing was pending and nothing was sent.
pub async fn send_approval_request(bot: &Bot, chat_id: ChatId) -> Result<usize, String> {
    let store = ProposalsStore::new();
    let proposals = store.list_brain_dedup_proposals();
    if proposals.is_empty() {
        return Ok(0);
    }
    let text = format_proposal_message(&proposals);
    let keyboard = build_keyboard(&proposals);
    crate::channels::telegram::send::message_in_thread(bot, chat_id, None, text)
        .parse_mode(ParseMode::Html)
        .reply_markup(keyboard)
        .await
        .map_err(|e| format!("send dedup approval: {e}"))?;
    Ok(proposals.len())
}

/// Handle a dedup approval button tap. `rest` is the callback data after the
/// `dedup:` prefix. Applies or rejects the proposal(s), answers the callback
/// query, and edits the originating message with the outcome so the buttons
/// are no longer actionable.
pub async fn handle_callback(bot: &Bot, query: &CallbackQuery, rest: &str, brain_dir: &Path) {
    let action = match parse_callback(rest) {
        Some(a) => a,
        None => {
            let _ = bot.answer_callback_query(query.id.clone()).await;
            return;
        }
    };

    let tool = build_tool(brain_dir);
    let result: Result<String, String> = match action {
        DedupAction::Apply(id) => tool.apply_brain_dedup(&id),
        DedupAction::Reject(id) => tool.reject(&id, Some("rejected via Telegram")),
        DedupAction::ApplyAll => {
            let store = ProposalsStore::new();
            let ids: Vec<String> = store
                .list_brain_dedup_proposals()
                .into_iter()
                .map(|p| p.id)
                .collect();
            let mut applied = 0usize;
            let mut errors: Vec<String> = Vec::new();
            for id in ids {
                match tool.apply_brain_dedup(&id) {
                    Ok(_) => applied += 1,
                    Err(e) => errors.push(format!("{id}: {e}")),
                }
            }
            if errors.is_empty() {
                Ok(format!("Applied {applied} dedup proposal(s)."))
            } else {
                Err(format!(
                    "Applied {applied}, failed {}: {}",
                    errors.len(),
                    errors.join("; ")
                ))
            }
        }
    };

    let summary = match result {
        Ok(s) => format!("\u{2705} {s}"),
        Err(e) => format!("\u{26a0}\u{fe0f} {e}"),
    };
    let _ = bot.answer_callback_query(query.id.clone()).await;

    if let Some(msg) = query.message.as_ref() {
        let _ = bot
            .edit_message_text(msg.chat().id, msg.id(), summary)
            .parse_mode(ParseMode::Html)
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_apply_single() {
        assert_eq!(
            parse_callback("apply:abc123"),
            Some(DedupAction::Apply("abc123".to_string()))
        );
    }

    #[test]
    fn parse_reject_single() {
        assert_eq!(
            parse_callback("reject:xyz"),
            Some(DedupAction::Reject("xyz".to_string()))
        );
    }

    #[test]
    fn parse_apply_all() {
        assert_eq!(parse_callback("apply_all"), Some(DedupAction::ApplyAll));
    }

    #[test]
    fn parse_rejects_empty_id() {
        assert_eq!(parse_callback("apply:"), None);
        assert_eq!(parse_callback("reject:"), None);
    }

    #[test]
    fn parse_rejects_unknown_verb() {
        assert_eq!(parse_callback("approve:abc"), None);
        assert_eq!(parse_callback("garbage"), None);
        assert_eq!(parse_callback(""), None);
    }

    #[test]
    fn parse_id_with_colon_keeps_suffix() {
        // split_once only splits on the first ':', so ids that themselves
        // contain a colon survive intact.
        assert_eq!(
            parse_callback("apply:ab:cd"),
            Some(DedupAction::Apply("ab:cd".to_string()))
        );
    }
}
