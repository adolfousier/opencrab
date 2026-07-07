use crate::db::Database;
use crate::db::models::ChannelMessage;
use crate::db::repository::channel_message::*;
use tokio;

#[tokio::test]
async fn test_channel_message_crud() {
    let db = Database::connect_in_memory()
        .await
        .expect("Failed to create database");
    db.run_migrations().await.expect("Failed to run migrations");
    let repo = ChannelMessageRepository::new(db.pool().clone());

    let msg = ChannelMessage::new(
        "telegram".into(),
        "-100123456".into(),
        Some("Test Group".into()),
        "42".into(),
        "Alice".into(),
        "Hello world".into(),
        "text".into(),
        Some("101".into()),
    );

    repo.insert(&msg).await.expect("Failed to insert");

    let recent = repo
        .recent(Some("telegram"), "-100123456", 10, None, None)
        .await
        .expect("Failed to fetch recent");
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].content, "Hello world");

    let results = repo
        .search(Some("telegram"), Some("-100123456"), "Hello", 10, None)
        .await
        .expect("Failed to search");
    assert_eq!(results.len(), 1);

    let chats = repo
        .list_chats(Some("telegram"))
        .await
        .expect("Failed to list chats");
    assert_eq!(chats.len(), 1);
    assert_eq!(chats[0].message_count, 1);
}
