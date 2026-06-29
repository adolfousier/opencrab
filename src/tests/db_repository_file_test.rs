use super::*;
use crate::db::Database;
use crate::db::models::Session;
use crate::db::repository::SessionRepository;
use std::path::PathBuf;
use tokio;

#[tokio::test]
async fn test_file_crud() {
    let db = Database::connect_in_memory()
        .await
        .expect("Failed to create database");
    db.run_migrations().await.expect("Failed to run migrations");
    let session_repo = SessionRepository::new(db.pool().clone());
    let file_repo = FileRepository::new(db.pool().clone());

    // Create session first
    let session = Session::new(Some("Test".to_string()), Some("model".to_string()), None);
    session_repo
        .create(&session)
        .await
        .expect("Failed to create session");

    // Create file
    let file = File::new(session.id, PathBuf::from("/test/file.rs"), None);
    file_repo
        .create(&file)
        .await
        .expect("Failed to create file");

    // Read
    let found = file_repo.find_by_id(file.id).await.expect("Failed to find");
    assert!(found.is_some());
    assert_eq!(found.as_ref().unwrap().path, PathBuf::from("/test/file.rs"));

    // Update
    let mut updated = file.clone();
    updated.content = Some("Updated content".to_string());
    file_repo.update(&updated).await.expect("Failed to update");

    let found = file_repo.find_by_id(file.id).await.expect("Failed to find");
    assert_eq!(found.unwrap().content, Some("Updated content".to_string()));

    // Delete
    file_repo.delete(file.id).await.expect("Failed to delete");
    let found = file_repo.find_by_id(file.id).await.expect("Failed to find");
    assert!(found.is_none());
}

#[tokio::test]
async fn test_file_list_by_session() {
    let db = Database::connect_in_memory()
        .await
        .expect("Failed to create database");
    db.run_migrations().await.expect("Failed to run migrations");
    let session_repo = SessionRepository::new(db.pool().clone());
    let file_repo = FileRepository::new(db.pool().clone());

    let session = Session::new(Some("Test".to_string()), Some("model".to_string()), None);
    session_repo
        .create(&session)
        .await
        .expect("Failed to create session");

    // Create multiple files
    for i in 0..3 {
        let file = File::new(
            session.id,
            PathBuf::from(format!("/test/file{}.rs", i)),
            None,
        );
        file_repo
            .create(&file)
            .await
            .expect("Failed to create file");
    }

    let files = file_repo
        .list_by_session(session.id)
        .await
        .expect("Failed to list");
    assert_eq!(files.len(), 3);

    let count = file_repo
        .count_by_session(session.id)
        .await
        .expect("Failed to count");
    assert_eq!(count, 3);
}
