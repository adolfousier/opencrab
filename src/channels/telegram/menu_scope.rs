//! Classifying per-member menu scope failures (#839).
//!
//! Global `allowed_users` are admins allowed in every group, so their menus
//! must be cleared wherever they are members. But nothing tells us which
//! groups those are: the config records a global grant, not a roster.
//!
//! Telegram answers a `BotCommandScope::ChatMember` call for a user who is not
//! in that chat with an invalid-id error, and that is the only membership
//! signal available here. `getChatMember` fails the same way for a user the bot
//! has never seen in the chat, so probing first costs a round trip per user per
//! group and still errors. The failed call is the membership test.

/// Does this error mean "that user is not in this chat" rather than a fault?
///
/// Matched on the rendered error because teloxide surfaces these as opaque API
/// strings rather than typed variants.
pub fn means_not_a_member(err: &str) -> bool {
    const NOT_A_MEMBER: [&str; 3] = [
        "USER_ID_INVALID",
        "PARTICIPANT_ID_INVALID",
        "USER_NOT_PARTICIPANT",
    ];
    let upper = err.to_ascii_uppercase();
    NOT_A_MEMBER.iter().any(|code| upper.contains(code))
}
