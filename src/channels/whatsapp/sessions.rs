//! Session to chat-JID map so a finished background task can resume the
//! chat that started it (#731). WhatsApp keeps no other session-to-target
//! map; the handler registers the pair on every turn.

use uuid::Uuid;

use super::WhatsAppState;

impl WhatsAppState {
    /// Map a session to the chat JID it is being handled in, so a finished
    /// background task can resume that chat (#731). Called on each turn.
    pub async fn register_session_jid(&self, session_id: Uuid, jid: String) {
        self.session_jids.lock().await.insert(session_id, jid);
    }

    /// The chat JID a session was last handled in, if known (#731).
    pub async fn session_jid(&self, session_id: Uuid) -> Option<String> {
        self.session_jids.lock().await.get(&session_id).cloned()
    }
}
