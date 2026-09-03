//! Tool-approval choices and the per-phone pending-approval register.
//!
//! While an approval is in flight, the next message from that phone (text or
//! button tap) is interpreted as a [`WaApproval`] instead of being routed to
//! the agent; `handler` resolves it through [`WhatsAppState::resolve_pending_approval`].

use super::WhatsAppState;

/// Approval choices mirroring the TUI's Yes / Always (session) / YOLO (permanent) / No.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaApproval {
    /// Approve this tool call once.
    Yes,
    /// Approve this and all future tool calls for the rest of the session.
    Always,
    /// Approve permanently (survives restarts).
    Yolo,
    /// Deny this tool call.
    No,
}

impl WhatsAppState {
    /// Register a pending approval for a phone number.
    pub async fn register_pending_approval(
        &self,
        phone: String,
        tx: tokio::sync::oneshot::Sender<WaApproval>,
    ) {
        self.pending_approvals.lock().await.insert(phone, tx);
    }

    /// Resolve a pending approval (called when user replies or taps a button).
    /// Returns `Some(choice)` if there was a pending approval, `None` otherwise.
    pub async fn resolve_pending_approval(
        &self,
        phone: &str,
        choice: WaApproval,
    ) -> Option<WaApproval> {
        if let Some(tx) = self.pending_approvals.lock().await.remove(phone) {
            let _ = tx.send(choice);
            Some(choice)
        } else {
            None
        }
    }
}
