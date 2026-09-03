//! Broadcast/subscribe pairs the onboarding UI listens on: QR codes,
//! connected, errors and delivered-message ids.
//!
//! Delivered ids come from `ReceiptType::Delivered` receipts, so the
//! onboarding connection test confirms only when a message actually reached
//! WhatsApp rather than when the send stanza was merely transmitted.

use tokio::sync::broadcast::Receiver;

use super::WhatsAppState;

impl WhatsAppState {
    /// Broadcast a connected event to any subscribed onboarding UI.
    pub fn broadcast_connected(&self) {
        let _ = self.connected_tx.send(());
    }

    /// Subscribe to QR code events (used by onboarding).
    pub fn subscribe_qr(&self) -> Receiver<String> {
        self.qr_tx.subscribe()
    }

    /// Subscribe to connection events (used by onboarding).
    pub fn subscribe_connected(&self) -> Receiver<()> {
        self.connected_tx.subscribe()
    }

    /// Broadcast an error to any subscribed onboarding UI.
    pub fn broadcast_error(&self, msg: &str) {
        let _ = self.error_tx.send(msg.to_string());
    }

    /// Subscribe to error events (used by onboarding).
    pub fn subscribe_error(&self) -> Receiver<String> {
        self.error_tx.subscribe()
    }

    /// Announce that a sent message id received a `Delivered` receipt. Called
    /// from the agent event loop so the onboarding test can confirm real
    /// delivery rather than mere transmission.
    pub fn broadcast_delivered(&self, message_id: &str) {
        let _ = self.delivered_tx.send(message_id.to_string());
    }

    /// Subscribe to delivered-message ids (used by the connection test).
    pub fn subscribe_delivered(&self) -> Receiver<String> {
        self.delivered_tx.subscribe()
    }
}
