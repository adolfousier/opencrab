//! /cowork Command: Create a cowork workspace with QR invite.
//!
//! Flow:
//! 1. User sends `/cowork` in DM → bot asks for workspace name
//! 2. User sends workspace name → bot generates `?startgroup=cowork_<id>` deep link
//! 3. User taps link → Telegram native UI creates group, adds bot
//! 4. Bot detects `/start cowork_<id>` in new group → generates invite link + QR
//! 5. QR sent to user's DM. New members auto-register in allowed_users.

use crate::config::{Config, opencrabs_home};

use super::TelegramState;
use super::send::{message_in_thread, photo_in_thread};
use teloxide::prelude::*;
use teloxide::types::{ChatId, InlineKeyboardButton, InlineKeyboardMarkup, InputFile};

/// Prefix for cowork startgroup parameters.
const COWORK_PREFIX: &str = "cowork_";

/// State for an active /cowork conversation.
#[derive(Debug, Clone)]
pub struct CoworkState {
    /// User who initiated /cowork.
    pub user_id: i64,
    /// DM chat where /cowork was sent (for sending QR back).
    pub chat_id: i64,
    /// Workspace name provided by user.
    pub workspace_name: String,
    /// Unique session identifier for this cowork flow.
    pub session_id: String,
}

impl CoworkState {
    pub fn new(user_id: i64, chat_id: i64, session_id: String) -> Self {
        Self {
            user_id,
            chat_id,
            workspace_name: String::new(),
            session_id,
        }
    }
}

/// Check if a `/start` parameter is a cowork session.
pub fn is_cowork_session(param: &str) -> bool {
    param.starts_with(COWORK_PREFIX) && param.len() > COWORK_PREFIX.len()
}

/// Extract the session ID from a cowork startgroup parameter.
/// "cowork_abc123" → Some("abc123"), "other" → None.
pub fn parse_startgroup_param(param: &str) -> Option<&str> {
    if is_cowork_session(param) {
        Some(&param[COWORK_PREFIX.len()..])
    } else {
        None
    }
}

/// Build a Telegram deep link that opens the "create group with bot" UI.
/// Format: `https://t.me/{bot_username}?startgroup=cowork_{session_id}`
pub fn build_cowork_deep_link(bot_username: &str, session_id: &str) -> String {
    format!(
        "https://t.me/{}?startgroup={}{}",
        bot_username, COWORK_PREFIX, session_id
    )
}

/// Generate a QR code PNG from an invite link. Returns (png_bytes, file_path).
/// Reuses `render_qr_png` from whatsapp_connect.
pub fn build_invite_qr(invite_link: &str) -> Option<(Vec<u8>, std::path::PathBuf)> {
    let png_bytes = crate::brain::tools::whatsapp_connect::render_qr_png(invite_link)?;
    let dir = opencrabs_home().join("tmp");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("cowork_invite_qr.png");
    std::fs::write(&path, &png_bytes).ok()?;
    Some((png_bytes, path))
}

/// Append a user_id to config.telegram.allowed_users if not already present.
/// Returns Ok(true) if newly registered, Ok(false) if already existed.
pub fn auto_register_user(user_id: i64) -> Result<bool, String> {
    let config = Config::load().map_err(|e| format!("Failed to load config: {e}"))?;

    let id_str = user_id.to_string();
    if config.channels.telegram.allowed_users.contains(&id_str) {
        return Ok(false);
    }

    let mut users = config.channels.telegram.allowed_users.clone();
    users.push(id_str);

    Config::write_array("channels.telegram", "allowed_users", &users)
        .map_err(|e| format!("Failed to write allowed_users: {e}"))?;

    Ok(true)
}

/// Check if a chat_id is a tracked cowork group.
pub async fn is_cowork_group(chat_id: i64, state: &TelegramState) -> bool {
    state.is_cowork_group(chat_id).await
}

/// Handle the /cowork command in DM. Returns the text to send back.
pub async fn handle_cowork_command(
    bot: &Bot,
    _msg: &Message,
    state: &TelegramState,
    user_id: i64,
    chat_id: i64,
    thread_id: Option<teloxide::types::ThreadId>,
) -> Result<(), teloxide::RequestError> {
    // Check if user already has an active cowork conversation
    if let Some(existing) = state.get_cowork_state(user_id).await {
        // User already in a cowork flow — show the deep link again
        let bot_username = state
            .bot_username()
            .await
            .unwrap_or_else(|| "opencrabsbot".to_string());
        let deep_link = build_cowork_deep_link(&bot_username, &existing.session_id);

        let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::url(
            "🦀 Add to Group".to_string(),
            deep_link.parse().unwrap(),
        )]]);

        let text = if existing.workspace_name.is_empty() {
            "You have a pending /cowork setup. What should we call your workspace?".to_string()
        } else {
            format!(
                "Workspace: **{}**\n\nTap below to add me to any group with your friends.\n\
                 Don't have one yet? Create a group in Telegram, invite your friends, then come back and tap this button.",
                existing.workspace_name
            )
        };

        message_in_thread(bot, ChatId(chat_id), thread_id, text)
            .reply_markup(keyboard)
            .await?;
        return Ok(());
    }

    // Start new cowork conversation
    let session_id = uuid::Uuid::new_v4().to_string()[..8].to_string();
    state.start_cowork(user_id, chat_id, session_id).await;

    message_in_thread(
        bot,
        ChatId(chat_id),
        thread_id,
        "🦀 **Cowork Setup**\n\nWhat should we call your workspace?",
    )
    .await?;
    Ok(())
}

/// Handle a user providing their workspace name (second message in /cowork flow).
pub async fn handle_workspace_name(
    bot: &Bot,
    state: &TelegramState,
    user_id: i64,
    chat_id: i64,
    workspace_name: &str,
    thread_id: Option<teloxide::types::ThreadId>,
) -> Result<(), teloxide::RequestError> {
    let Some(mut cowork) = state.set_workspace_name(user_id, workspace_name).await else {
        return Ok(());
    };
    cowork.workspace_name = workspace_name.to_string();

    let bot_username = state
        .bot_username()
        .await
        .unwrap_or_else(|| "opencrabsbot".to_string());
    let deep_link = build_cowork_deep_link(&bot_username, &cowork.session_id);

    let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::url(
        "🦀 Add to Group".to_string(),
        deep_link.parse().unwrap(),
    )]]);

    let text = format!(
        "Workspace: **{}**\n\nTap below to add me to any group with your friends.\n\
         Don't have one yet? Create a group in Telegram, invite your friends, then come back and tap this button.\n\n\
         I'll auto-register everyone when they send a message.",
        workspace_name
    );

    message_in_thread(bot, ChatId(chat_id), thread_id, text)
        .reply_markup(keyboard)
        .await?;
    Ok(())
}

/// Handle the bot being added to a group via `?startgroup=cowork_<id>`.
pub async fn handle_cowork_group_join(
    bot: &Bot,
    msg: &Message,
    state: &TelegramState,
    param: &str,
    thread_id: Option<teloxide::types::ThreadId>,
) -> Result<(), teloxide::RequestError> {
    let Some(session_id) = parse_startgroup_param(param) else {
        return Ok(());
    };

    let group_chat_id = msg.chat.id.0;

    // Look up the cowork state by session_id
    let cowork = state.take_cowork_by_session(session_id).await;

    // Track this group as a cowork group
    state.add_cowork_group(group_chat_id).await;

    // Generate invite link
    let invite_result = bot.create_chat_invite_link(ChatId(group_chat_id)).await;

    match invite_result {
        Ok(link) => {
            let invite_url = &link.invite_link;

            // Generate QR
            if let Some((_png_bytes, qr_path)) = build_invite_qr(invite_url) {
                // Send QR to user's DM (not to the group)
                if let Some(ref cowork_state) = cowork {
                    let user_chat = ChatId(cowork_state.chat_id);
                    let _ = photo_in_thread(bot, user_chat, None, InputFile::file(qr_path)).await;
                    let _ = message_in_thread(
                        bot,
                        user_chat,
                        None,
                        format!(
                            "🦀 **Workspace created!**\n\n\
                             Group: **{}**\n\
                             Invite link: {}\n\n\
                             Share the QR or link with your team. \
                             Their IDs auto-register when they join.",
                            cowork_state.workspace_name, invite_url
                        ),
                    )
                    .await;
                }
            }

            // Send welcome in the group
            let _ = message_in_thread(
                bot,
                ChatId(group_chat_id),
                thread_id,
                "🦀 Welcome to your Cowork workspace!\n\n\
                 Share the invite link with your team. \
                 @mention me anytime to chat.",
            )
            .await;
        }
        Err(e) => {
            tracing::error!("Cowork: failed to create invite link: {}", e);
            let _ = message_in_thread(
                bot,
                ChatId(group_chat_id),
                thread_id,
                "Failed to generate invite link. Make sure I'm an admin in this group.",
            )
            .await;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_startgroup_valid() {
        assert_eq!(parse_startgroup_param("cowork_abc123"), Some("abc123"));
    }

    #[test]
    fn parse_startgroup_not_cowork() {
        assert_eq!(parse_startgroup_param("other_param"), None);
    }

    #[test]
    fn parse_startgroup_empty() {
        assert_eq!(parse_startgroup_param(""), None);
    }

    #[test]
    fn parse_startgroup_just_prefix() {
        assert_eq!(parse_startgroup_param("cowork_"), None);
    }

    #[test]
    fn is_cowork_session_true() {
        assert!(is_cowork_session("cowork_xxx"));
    }

    #[test]
    fn is_cowork_session_false() {
        assert!(!is_cowork_session("other"));
    }

    #[test]
    fn build_deep_link_format() {
        let link = build_cowork_deep_link("mybot", "abc123");
        assert_eq!(link, "https://t.me/mybot?startgroup=cowork_abc123");
    }

    #[test]
    fn build_deep_link_with_bot_suffix() {
        let link = build_cowork_deep_link("team_crab_bot", "xyz");
        assert_eq!(link, "https://t.me/team_crab_bot?startgroup=cowork_xyz");
    }

    #[test]
    fn cowork_state_lifecycle() {
        let state = CoworkState::new(123, 456, "abc".to_string());
        assert_eq!(state.user_id, 123);
        assert_eq!(state.chat_id, 456);
        assert!(state.workspace_name.is_empty());
        assert_eq!(state.session_id, "abc");
    }

    #[test]
    fn invite_qr_generation() {
        let result = build_invite_qr("https://t.me/+AbCdEfGh");
        assert!(result.is_some());
        let (bytes, path) = result.unwrap();
        // PNG magic number
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        assert!(path.exists());
        // Cleanup
        let _ = std::fs::remove_file(path);
    }
}
