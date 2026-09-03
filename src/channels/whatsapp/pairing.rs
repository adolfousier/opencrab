//! QR pairing lifecycle: broadcast + replay, fresh-pairing restart and the
//! one-shot first-pair flag that gates the connection greeting.
//!
//! The QR channel is a plain broadcast with no replay, so the last code is
//! remembered and replayed to late subscribers (the "press Enter twice"
//! bug). Once connected the QR is locked so a stale code never reappears in
//! the onboarding UI.

use std::sync::atomic::Ordering;

use super::WhatsAppState;

impl WhatsAppState {
    /// Broadcast a QR code to any subscribed onboarding UI, and remember it so
    /// a subscriber that joins after this point can replay it immediately.
    ///
    /// No-op once connected: after pairing succeeds the QR is locked, so a late
    /// or stale QR event can never reappear in the onboarding UI.
    pub fn broadcast_qr(&self, code: &str) {
        if self.connected.load(Ordering::SeqCst) {
            tracing::debug!("WhatsApp: suppressing QR broadcast — already connected");
            return;
        }
        *self.last_qr.lock().unwrap_or_else(|e| e.into_inner()) = Some(code.to_string());
        let _ = self.qr_tx.send(code.to_string());
    }

    /// The most recently broadcast QR, if any. Replayed to a new subscriber so
    /// it does not have to wait for the next refresh (fixes the "Enter twice"
    /// race). Cleared on [`Self::request_restart`] and on connect.
    pub fn current_qr(&self) -> Option<String> {
        self.last_qr
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Request a fresh pairing: the next `reconcile_whatsapp` aborts the live
    /// agent and starts a new one against the wiped session. Clears the stored
    /// QR so the stale one is never replayed, and clears the connected flag so
    /// a new QR can be broadcast again for the re-pairing.
    pub fn request_restart(&self) {
        *self.last_qr.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.connected.store(false, Ordering::SeqCst);
        self.restart_requested.store(true, Ordering::SeqCst);
    }

    /// Consume the restart request (returns whether one was pending).
    pub fn take_restart_request(&self) -> bool {
        self.restart_requested.swap(false, Ordering::SeqCst)
    }

    /// Mark that a fresh pairing just succeeded. The next `Connected` event
    /// will fire the one-time connection greeting.
    pub fn set_first_pair_pending(&self) {
        self.first_pair_pending.store(true, Ordering::SeqCst);
    }

    /// Consume the first-pair flag. Returns `true` exactly once per pairing
    /// (first-time or re-pair after reset), then resets to `false` so a plain
    /// app restart never re-triggers the greeting.
    pub fn take_first_pair_pending(&self) -> bool {
        self.first_pair_pending.swap(false, Ordering::SeqCst)
    }
}
