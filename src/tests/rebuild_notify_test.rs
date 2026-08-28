//! Tests for rebuild outcome delivery (#304/#305): the background build's
//! completion and failure notices must reach whoever asked — a chat via the
//! cron `deliver_to` path, the TUI via the scheduler's session notifier.

use crate::brain::tools::rebuild::rebuild_deliver_target;
use crate::db::Database;
use crate::db::repository::PendingRequestRepository;

// ── deliver_to mapping (#305) ────────────────────────────────────────────

#[test]
fn channel_chats_map_to_delivery_targets() {
    assert_eq!(
        rebuild_deliver_target("telegram", Some("123456789")).as_deref(),
        Some("telegram:123456789")
    );
    assert_eq!(
        rebuild_deliver_target("discord", Some("987654321")).as_deref(),
        Some("discord:987654321")
    );
    assert_eq!(
        rebuild_deliver_target("slack", Some("C0123ABC")).as_deref(),
        Some("slack:C0123ABC")
    );
}

#[test]
fn tui_and_unsupported_channels_map_to_none() {
    // TUI has its own notifier; whatsapp has no cron delivery arm.
    assert!(rebuild_deliver_target("tui", None).is_none());
    assert!(rebuild_deliver_target("tui", Some("ignored")).is_none());
    assert!(rebuild_deliver_target("whatsapp", Some("15551234567")).is_none());
}

#[test]
fn missing_or_empty_chat_id_maps_to_none() {
    assert!(rebuild_deliver_target("telegram", None).is_none());
    assert!(rebuild_deliver_target("telegram", Some("")).is_none());
    assert!(rebuild_deliver_target("telegram", Some("   ")).is_none());
}

// ── pending-request lookup (#305 source of channel/chat) ────────────────

#[tokio::test]
async fn find_latest_for_session_returns_the_current_turn_row() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let repo = PendingRequestRepository::new(db.pool().clone());

    let session_id = uuid::Uuid::new_v4();
    repo.insert(
        uuid::Uuid::new_v4(),
        session_id,
        "please rebuild yourself",
        "telegram",
        Some("123456789"),
        "user",
    )
    .await
    .unwrap();

    let row = repo
        .find_latest_for_session(session_id)
        .await
        .unwrap()
        .expect("in-flight row must be found");
    assert_eq!(row.channel, "telegram");
    assert_eq!(row.channel_chat_id.as_deref(), Some("123456789"));

    // End-to-end mapping: this is the deliver_to the tool will schedule.
    assert_eq!(
        rebuild_deliver_target(&row.channel, row.channel_chat_id.as_deref()).as_deref(),
        Some("telegram:123456789")
    );
}

#[tokio::test]
async fn find_latest_for_session_none_when_no_row() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let repo = PendingRequestRepository::new(db.pool().clone());

    let row = repo
        .find_latest_for_session(uuid::Uuid::new_v4())
        .await
        .unwrap();
    assert!(row.is_none());
}
