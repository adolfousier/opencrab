//! Notify receipts (fork #50): queued to injected, target-scoped stamping, idempotent drain.

use crate::brain::agent::service::notify_receipts::*;
use uuid::Uuid;

#[test]
fn receipt_lifecycle_queued_then_injected() {
    let (id, target) = (Uuid::new_v4(), Uuid::new_v4());
    record_queued(id, target);
    let receipt = status(id).expect("receipt recorded");
    assert_eq!(receipt.state, ReceiptState::Queued);
    assert_eq!(receipt.target, target);
    assert!(receipt.injected_at.is_none());

    let stamped = mark_injected_for_target(target);
    assert_eq!(stamped, 1);
    let receipt = status(id).expect("receipt survives stamping");
    assert_eq!(receipt.state, ReceiptState::Injected);
    assert!(receipt.injected_at.is_some());
}

#[test]
fn injection_stamp_is_target_scoped() {
    let (id_a, id_b) = (Uuid::new_v4(), Uuid::new_v4());
    let (target_a, target_b) = (Uuid::new_v4(), Uuid::new_v4());
    record_queued(id_a, target_a);
    record_queued(id_b, target_b);

    assert_eq!(mark_injected_for_target(target_a), 1);
    assert_eq!(status(id_a).unwrap().state, ReceiptState::Injected);
    assert_eq!(status(id_b).unwrap().state, ReceiptState::Queued);
}

#[test]
fn status_of_unknown_id_is_none() {
    assert!(status(Uuid::new_v4()).is_none());
}

#[test]
fn drain_is_idempotent_per_receipt() {
    let (id, target) = (Uuid::new_v4(), Uuid::new_v4());
    record_queued(id, target);
    assert_eq!(mark_injected_for_target(target), 1);
    // A second drain (next tool iteration) must not resurrect or
    // double-count the already-injected receipt.
    assert_eq!(mark_injected_for_target(target), 0);
    let receipt = status(id).unwrap();
    assert_eq!(receipt.state, ReceiptState::Injected);
}
