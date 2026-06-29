use super::*;
use crate::db::Database;
use crate::db::models::Session;
use crate::db::repository::SessionRepository;
use tokio;

#[tokio::test]
async fn test_message_crud() {
    let db = Database::connect_in_memory()
        .await
        .expect("Failed to create database");
    db.run_migrations().await.expect("Failed to run migrations");
    let session_repo = SessionRepository::new(db.pool().clone());
    let message_repo = MessageRepository::new(db.pool().clone());

    // Create session first
    let session = Session::new(Some("Test".to_string()), Some("model".to_string()), None);
    session_repo
        .create(&session)
        .await
        .expect("Failed to create session");

    // Create message
    let message = Message::new(session.id, "user".to_string(), "Hello!".to_string(), 1);
    message_repo
        .create(&message)
        .await
        .expect("Failed to create message");

    // Read
    let found = message_repo
        .find_by_id(message.id)
        .await
        .expect("Failed to find");
    assert!(found.is_some());
    assert_eq!(found.unwrap().content, "Hello!");

    // Update
    let mut updated = message.clone();
    updated.content = "Updated content".to_string();
    message_repo
        .update(&updated)
        .await
        .expect("Failed to update");

    let found = message_repo
        .find_by_id(message.id)
        .await
        .expect("Failed to find");
    assert_eq!(found.unwrap().content, "Updated content");

    // Delete
    message_repo
        .delete(message.id)
        .await
        .expect("Failed to delete");
    let found = message_repo
        .find_by_id(message.id)
        .await
        .expect("Failed to find");
    assert!(found.is_none());
}

#[tokio::test]
async fn test_message_list_by_session() {
    let db = Database::connect_in_memory()
        .await
        .expect("Failed to create database");
    db.run_migrations().await.expect("Failed to run migrations");
    let session_repo = SessionRepository::new(db.pool().clone());
    let message_repo = MessageRepository::new(db.pool().clone());

    let session = Session::new(Some("Test".to_string()), Some("model".to_string()), None);
    session_repo
        .create(&session)
        .await
        .expect("Failed to create session");

    // Create multiple messages
    for i in 0..3 {
        let msg = Message::new(
            session.id,
            "user".to_string(),
            format!("Message {}", i),
            i + 1,
        );
        message_repo
            .create(&msg)
            .await
            .expect("Failed to create message");
    }

    let messages = message_repo
        .list_by_session(session.id)
        .await
        .expect("Failed to list");
    assert_eq!(messages.len(), 3);

    let count = message_repo
        .count_by_session(session.id)
        .await
        .expect("Failed to count");
    assert_eq!(count, 3);
}
