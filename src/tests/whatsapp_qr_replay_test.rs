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
