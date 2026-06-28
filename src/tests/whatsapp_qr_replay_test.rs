//! WhatsApp QR replay + restart request (#240).
//!
//! The QR channel is a plain broadcast with no replay, so a connect flow that
//! subscribed after the agent emitted its QR saw nothing until the next refresh
//! (the "press Enter twice" bug). And a reset must drop old auth at runtime, so
//! it requests an agent restart. These pin both behaviors.

use crate::channels::whatsapp::WhatsAppState;

#[test]
fn current_qr_replays_the_last_broadcast() {
    let s = WhatsAppState::new();
    assert_eq!(s.current_qr(), None, "nothing broadcast yet");
    s.broadcast_qr("QR-ABC");
    assert_eq!(s.current_qr().as_deref(), Some("QR-ABC"));
    s.broadcast_qr("QR-DEF");
    assert_eq!(s.current_qr().as_deref(), Some("QR-DEF"), "latest wins");
}

#[test]
fn request_restart_sets_flag_and_clears_stale_qr() {
    let s = WhatsAppState::new();
    s.broadcast_qr("QR-OLD");
    s.request_restart();
    assert_eq!(
        s.current_qr(),
        None,
        "stale QR must be cleared so it is not replayed after a reset"
    );
    assert!(s.take_restart_request(), "a restart is pending");
    assert!(
        !s.take_restart_request(),
        "the request is consumed exactly once"
    );
}

#[test]
fn no_restart_pending_by_default() {
    let s = WhatsAppState::new();
    assert!(!s.take_restart_request());
}

/// Once connected the QR is locked: a late/stale `broadcast_qr` is a no-op so a
/// QR can never reappear after the account is linked. A reset re-enables it.
///
/// `mark_connected` is the testable core of `set_connected` (the latter needs a
/// live `Arc<Client>` that a unit test cannot construct); both flip the same
/// connected flag and clear any stale QR.
#[tokio::test]
async fn qr_is_locked_after_connect_and_reenabled_on_restart() {
    let s = WhatsAppState::new();
    assert!(!s.is_connected().await, "not connected initially");

    s.broadcast_qr("QR-PRECONNECT");
    assert_eq!(s.current_qr().as_deref(), Some("QR-PRECONNECT"));

    // Connecting flips the flag and drops the stale QR.
    s.mark_connected();
    assert!(s.is_connected().await, "connected after mark_connected");
    assert_eq!(s.current_qr(), None, "connecting clears the stale QR");

    // A late/stale QR event after connect is suppressed entirely.
    s.broadcast_qr("QR-STALE");
    assert_eq!(s.current_qr(), None, "no QR may reappear once connected");

    // A reset re-enables pairing: connected flips back to false and a fresh QR
    // can be broadcast again.
    s.request_restart();
    assert!(!s.is_connected().await, "restart clears the connected flag");
    s.broadcast_qr("QR-REPAIR");
    assert_eq!(s.current_qr().as_deref(), Some("QR-REPAIR"));
}
