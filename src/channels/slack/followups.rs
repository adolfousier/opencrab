//! Optional follow-up suggestions from `suggest_options` (#599).
//!
//! Non-blocking: buttons ride under the response and a tap injects the
//! chosen suggestion as a new turn. The action id only carries the option
//! index; the tap handler maps it back through the stored list. Cleared on
//! tap or when the user sends anything of their own.

use uuid::Uuid;

use super::SlackState;

impl SlackState {
    /// Stash this session's optional follow-up suggestions (#599).
    pub async fn set_pending_followups(&self, session_id: Uuid, options: Vec<String>) {
        self.pending_followups
            .lock()
            .await
            .insert(session_id, options);
    }

    /// Take a tapped follow-up suggestion by index, consuming the whole set.
    pub async fn take_pending_followup(&self, session_id: Uuid, idx: usize) -> Option<String> {
        let options = self.pending_followups.lock().await.remove(&session_id)?;
        options.get(idx).cloned()
    }

    /// Drop this session's pending follow-up suggestions (user sent their own).
    pub async fn clear_pending_followups(&self, session_id: Uuid) {
        self.pending_followups.lock().await.remove(&session_id);
    }
}
