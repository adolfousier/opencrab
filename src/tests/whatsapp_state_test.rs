//! Tests for WhatsApp state broadcasting (QR, connected, error channels).

use crate::channels::whatsapp::WhatsAppState;

#[test]
fn broadcast_qr_received_by_subscriber() {
    let state = WhatsAppState::new();
    let mut rx = state.subscribe_qr();
    state.broadcast_qr("test-qr-code");
    let received = rx.try_recv().unwrap();
    assert_eq!(received, "test-qr-code");
}

#[test]
fn broadcast_connected_received_by_subscriber() {
    let state = WhatsAppState::new();
    let mut rx = state.subscribe_connected();
    state.broadcast_connected();
    assert!(rx.try_recv().is_ok());
}

#[test]
fn broadcast_error_received_by_subscriber() {
    let state = WhatsAppState::new();
    let mut rx = state.subscribe_error();
    state.broadcast_error("session store failed");
    let received = rx.try_recv().unwrap();
    assert_eq!(received, "session store failed");
}

#[test]
fn multiple_qr_codes_received_in_order() {
    let state = WhatsAppState::new();
    let mut rx = state.subscribe_qr();
    state.broadcast_qr("qr-1");
    state.broadcast_qr("qr-2");
    state.broadcast_qr("qr-3");
    assert_eq!(rx.try_recv().unwrap(), "qr-1");
    assert_eq!(rx.try_recv().unwrap(), "qr-2");
    assert_eq!(rx.try_recv().unwrap(), "qr-3");
}

#[test]
fn no_subscriber_does_not_panic() {
    // Broadcasting without any subscriber should not panic
    let state = WhatsAppState::new();
    state.broadcast_qr("no one listening");
    state.broadcast_connected();
    state.broadcast_error("no one listening");
}

#[test]
fn error_channel_independent_of_qr_channel() {
    let state = WhatsAppState::new();
    let mut qr_rx = state.subscribe_qr();
    let mut err_rx = state.subscribe_error();

    state.broadcast_error("something broke");

    // QR channel should be empty
    assert!(qr_rx.try_recv().is_err());
    // Error channel should have the message
    assert_eq!(err_rx.try_recv().unwrap(), "something broke");
}

#[test]
fn late_subscriber_misses_earlier_messages() {
    let state = WhatsAppState::new();
    state.broadcast_qr("before-subscribe");
    let mut rx = state.subscribe_qr();
    assert!(rx.try_recv().is_err());
}

/// #731: the session→JID map lets a finished background task resume the right
/// chat. Unknown sessions resolve to None; the latest registration wins.
#[tokio::test]
async fn session_jid_registers_and_resolves() {
    let state = WhatsAppState::new();
    let sid = uuid::Uuid::new_v4();
    assert!(
        state.session_jid(sid).await.is_none(),
        "an unregistered session has no jid"
    );
    state
        .register_session_jid(sid, "123@s.whatsapp.net".to_string())
        .await;
    assert_eq!(
        state.session_jid(sid).await.as_deref(),
        Some("123@s.whatsapp.net")
    );
    // The chat can move; the latest registration wins.
    state
        .register_session_jid(sid, "456@s.whatsapp.net".to_string())
        .await;
    assert_eq!(
        state.session_jid(sid).await.as_deref(),
        Some("456@s.whatsapp.net")
    );
}
