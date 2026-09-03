//! Connected client, owner JID and the connected flag.
//!
//! Set when the bot connects (static agent or the `whatsapp_connect` tool),
//! read by `whatsapp_send` to message the owner on demand.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use whatsapp_rust::client::Client;

use super::WhatsAppState;

impl WhatsAppState {
    /// Store the connected client and owner JID, then mark connected (which
    /// locks the QR) and notify onboarding subscribers.
    pub async fn set_connected(&self, client: Arc<Client>, owner_jid: Option<String>) {
        *self.client.lock().await = Some(client);
        if let Some(jid) = owner_jid {
            *self.owner_jid.lock().await = Some(jid);
        }
        self.mark_connected();
        self.broadcast_connected();
    }

    /// Flip the connected flag and drop any stale QR so it can never be
    /// replayed after pairing. Shared core of [`Self::set_connected`]; exposed
    /// separately because unit tests cannot construct a live `Client`.
    pub fn mark_connected(&self) {
        self.connected.store(true, Ordering::SeqCst);
        *self.last_qr.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// Record the freshly-paired owner's JID (`<number>@s.whatsapp.net`).
    /// Called on `PairSuccess` so the subsequent `Connected` handler and the
    /// `whatsapp_send` tool address the right account even on a first pairing
    /// where the startup-derived owner was unknown.
    pub async fn set_owner_jid(&self, jid: String) {
        *self.owner_jid.lock().await = Some(jid);
    }

    /// Get a clone of the connected client, if any.
    pub async fn client(&self) -> Option<Arc<Client>> {
        self.client.lock().await.clone()
    }

    /// Get the owner's JID for proactive messaging.
    pub async fn owner_jid(&self) -> Option<String> {
        self.owner_jid.lock().await.clone()
    }

    /// Check if WhatsApp is currently connected (reflects the connected flag set
    /// on pairing/connect and cleared on `request_restart`).
    pub async fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }
}
