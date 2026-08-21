//! Where a cron turn is allowed to send, and nowhere else.
//!
//! A cron job runs with no channel origin: nothing binds its session to a
//! chat. The proactive send path resolves a destination from the tool input
//! first, and a job's turn can pick that input up from anywhere it reads,
//! including a recalled memory. On 2026-08-21 a memory note from two weeks
//! earlier recorded a chat id and thread id under the heading "CONTINUE THIS
//! TASK", and a job posted its report into that group — one it was never
//! configured for, whose members had asked nothing.
//!
//! The rule this enforces: a cron turn may send only to the target its job was
//! created with. With no target it sends to no chat at all, and its output
//! lives in its own session. Recalled text can shape what a report says; it can
//! never decide where the report goes.

tokio::task_local! {
    /// The only chat this cron turn may send to. Present but `None` means the
    /// job set no `deliver_to`: it may send nowhere.
    static CRON_SEND_TARGET: Option<i64>;
}

/// Run `fut` with cron send scoping active, permitting `target` and nothing
/// else. Task-local, so it covers every await inside the turn and never
/// reaches a sibling job on the scheduler.
pub async fn with_send_target<F, T>(target: Option<i64>, fut: F) -> T
where
    F: std::future::Future<Output = T>,
{
    CRON_SEND_TARGET.scope(target, fut).await
}

/// What this turn may do with a proactive send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendPermission {
    /// Not a cron turn. The ordinary channel rules apply.
    Unscoped,
    /// A cron turn whose job named this chat.
    OnlyChat(i64),
    /// A cron turn whose job named no target.
    Nowhere,
}

/// The permission in force for the current task.
pub fn permission() -> SendPermission {
    CRON_SEND_TARGET
        .try_with(|target| match target {
            Some(chat) => SendPermission::OnlyChat(*chat),
            None => SendPermission::Nowhere,
        })
        .unwrap_or(SendPermission::Unscoped)
}

/// May the current task send to `chat_id`?
///
/// Outside a cron turn this is always true: the rule exists to stop a job
/// reaching chats it was never given, not to police ordinary replies.
pub fn may_send_to(chat_id: i64) -> bool {
    match permission() {
        SendPermission::Unscoped => true,
        SendPermission::OnlyChat(allowed) => allowed == chat_id,
        SendPermission::Nowhere => false,
    }
}

/// Why a send was refused, for the tool result the model reads.
pub fn refusal(chat_id: i64) -> String {
    match permission() {
        SendPermission::OnlyChat(allowed) => format!(
            "Refused: this scheduled job may only send to chat {allowed}, and this send \
             targeted {chat_id}. If the report belongs in another chat, change the job's \
             deliver_to; a chat id found in memory or in earlier context is not permission \
             to post there."
        ),
        _ => format!(
            "Refused: this scheduled job has no deliver_to, so it may not send to any chat \
             (attempted {chat_id}). Its output stays in its own session. Set deliver_to on \
             the job if it should report to a chat."
        ),
    }
}
