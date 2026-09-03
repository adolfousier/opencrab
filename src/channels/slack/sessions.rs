//! Session to channel-id map so approvals and resumed background results
//! route back to the channel the session was last handled in.

use uuid::Uuid;

use super::SlackState;

impl SlackState {
    /// Record which channel_id corresponds to a given session.
    pub async fn register_session_channel(&self, session_id: Uuid, channel_id: String) {
        self.session_channels
            .lock()
            .await
            .insert(session_id, channel_id);
    }

    /// Look up the channel_id for a session.
    pub async fn session_channel(&self, session_id: Uuid) -> Option<String> {
        self.session_channels.lock().await.get(&session_id).cloned()
    }
}
