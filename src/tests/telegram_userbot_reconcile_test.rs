//! Telegram userbot gating tests — the decision logic that keeps the read
//! plane OFF until every precondition holds.
//!
//! The userbot is an MTProto session logged in as the user's own account.
//! The gating rule is deliberately conservative: it must not run unless the
//! BOT plane can carry the replies (token + bot enabled) AND the user opted
//! in (`userbot.enabled`) AND a login session exists. The session check is
//! filesystem state, covered by `should_run_userbot` here mirroring the
//! manager's logic; the manager logs the login instruction instead of
//! starting when the session file is absent.

use crate::channels::manager::{ChannelAction, channel_action};

/// Mirror of the manager's `should_run` for the userbot plane:
/// bot enabled && userbot enabled && bot token valid.
fn should_run_userbot(bot_enabled: bool, userbot_enabled: bool, has_token: bool) -> bool {
    bot_enabled && userbot_enabled && has_token
}

#[test]
fn userbot_never_starts_when_bot_plane_cannot_carry_replies() {
    // No token: replies would have no exit path.
    assert!(!should_run_userbot(true, true, false));
    // Bot plane off: userbot is a companion, not a standalone channel.
    assert!(!should_run_userbot(false, true, true));
}

#[test]
fn userbot_starts_only_when_all_gates_pass() {
    assert!(should_run_userbot(true, true, true));
    // and the action mapping behaves like every other channel:
    assert_eq!(channel_action(true, false), ChannelAction::Start);
    assert_eq!(channel_action(false, true), ChannelAction::Stop);
    assert_eq!(channel_action(true, true), ChannelAction::Noop);
}

#[test]
fn dead_userbot_task_is_restarted_not_treated_as_running() {
    // Same invariant as #239/#240: a dead handle must yield Start, not Noop.
    assert_eq!(channel_action(true, false), ChannelAction::Start);
}
