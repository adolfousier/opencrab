use crate::services::ServiceContext;
use crate::services::SessionService;
use crate::services::file::*;
use std::path::PathBuf;

async fn create_test_service() -> (FileService, SessionService) {
    use crate::db::Database;

    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let pool = db.pool().clone();

    let context = ServiceContext::new(pool);
    (
        FileService::new(context.clone()),
        SessionService::new(context),
    )
}

#[tokio::test]
async fn test_track_file() {
    let (file_service, session_service) = create_test_service().await;
    let session = session_service
        .create_session(Some("Test".to_string()))
        .await
        .unwrap();

    let file = file_service
        .track_file(
            session.id,
            PathBuf::from("/test/file.txt"),
            Some("content".to_string()),
        )
        .await
        .unwrap();

    assert_eq!(file.session_id, session.id);
    assert_eq!(file.path, PathBuf::from("/test/file.txt"));
    assert_eq!(file.content, Some("content".to_string()));
}

#[tokio::test]
async fn test_get_file() {
    let (file_service, session_service) = create_test_service().await;
    let session = session_service
        .create_session(Some("Test".to_string()))
        .await
        .unwrap();

    let created = file_service
        .track_file(session.id, PathBuf::from("/test/file.txt"), None)
        .await
        .unwrap();

    let found = file_service.get_file(created.id).await.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().id, created.id);
}

#[tokio::test]
async fn test_list_files_for_session() {
    let (file_service, session_service) = create_test_service().await;
    let session = session_service
        .create_session(Some("Test".to_string()))
        .await
        .unwrap();

    file_service
        .track_file(session.id, PathBuf::from("/test/file1.txt"), None)
        .await
        .unwrap();
    file_service
        .track_file(session.id, PathBuf::from("/test/file2.txt"), None)
        .await
        .unwrap();

    let files = file_service
        .list_files_for_session(session.id)
        .await
        .unwrap();
    assert_eq!(files.len(), 2);
}

#[tokio::test]
async fn test_find_file_by_path() {
    let (file_service, session_service) = create_test_service().await;
    let session = session_service
        .create_session(Some("Test".to_string()))
        .await
        .unwrap();

    let path = PathBuf::from("/test/file.txt");
    file_service
        .track_file(session.id, path.clone(), None)
        .await
        .unwrap();

    let found = file_service
        .find_file_by_path(session.id, &path)
        .await
        .unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().path, path);
}

#[tokio::test]
async fn test_update_file_content() {
    let (file_service, session_service) = create_test_service().await;
    let session = session_service
        .create_session(Some("Test".to_string()))
        .await
        .unwrap();

    let file = file_service
        .track_file(session.id, PathBuf::from("/test/file.txt"), None)
        .await
        .unwrap();

    file_service
        .update_file_content(file.id, Some("new content".to_string()))
        .await
        .unwrap();

    let updated = file_service.get_file_required(file.id).await.unwrap();
    assert_eq!(updated.content, Some("new content".to_string()));
}

#[tokio::test]
async fn test_delete_file() {
    let (file_service, session_service) = create_test_service().await;
    let session = session_service
        .create_session(Some("Test".to_string()))
        .await
        .unwrap();

    let file = file_service
        .track_file(session.id, PathBuf::from("/test/file.txt"), None)
        .await
        .unwrap();

    file_service.delete_file(file.id).await.unwrap();

    let result = file_service.get_file(file.id).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_delete_files_for_session() {
    let (file_service, session_service) = create_test_service().await;
    let session = session_service
        .create_session(Some("Test".to_string()))
        .await
        .unwrap();

    file_service
        .track_file(session.id, PathBuf::from("/test/file1.txt"), None)
        .await
        .unwrap();
    file_service
        .track_file(session.id, PathBuf::from("/test/file2.txt"), None)
        .await
        .unwrap();

    file_service
        .delete_files_for_session(session.id)
        .await
        .unwrap();

    let files = file_service
        .list_files_for_session(session.id)
        .await
        .unwrap();
    assert_eq!(files.len(), 0);
}

#[tokio::test]
async fn test_count_files_in_session() {
    let (file_service, session_service) = create_test_service().await;
    let session = session_service
        .create_session(Some("Test".to_string()))
        .await
        .unwrap();

    file_service
        .track_file(session.id, PathBuf::from("/test/file1.txt"), None)
        .await
        .unwrap();
    file_service
        .track_file(session.id, PathBuf::from("/test/file2.txt"), None)
        .await
        .unwrap();

    let count = file_service
        .count_files_in_session(session.id)
        .await
        .unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn test_is_file_tracked() {
    let (file_service, session_service) = create_test_service().await;
    let session = session_service
        .create_session(Some("Test".to_string()))
        .await
        .unwrap();

    let path = PathBuf::from("/test/file.txt");
    file_service
        .track_file(session.id, path.clone(), None)
        .await
        .unwrap();

    let is_tracked = file_service
        .is_file_tracked(session.id, &path)
        .await
        .unwrap();
    assert!(is_tracked);

    let not_tracked = file_service
        .is_file_tracked(session.id, &PathBuf::from("/test/other.txt"))
        .await
        .unwrap();
    assert!(!not_tracked);
}

#[tokio::test]
async fn test_get_or_create_file() {
    let (file_service, session_service) = create_test_service().await;
    let session = session_service
        .create_session(Some("Test".to_string()))
        .await
        .unwrap();

    let path = PathBuf::from("/test/file.txt");

    // First call should create
    let file1 = file_service
        .get_or_create_file(session.id, path.clone(), Some("content".to_string()))
        .await
        .unwrap();

    // Second call should return existing
    let file2 = file_service
        .get_or_create_file(session.id, path.clone(), None)
        .await
        .unwrap();

    assert_eq!(file1.id, file2.id);
}

#[tokio::test]
async fn test_get_files_with_content() {
    let (file_service, session_service) = create_test_service().await;
    let session = session_service
        .create_session(Some("Test".to_string()))
        .await
        .unwrap();

    file_service
        .track_file(
            session.id,
            PathBuf::from("/test/file1.txt"),
            Some("content".to_string()),
        )
        .await
        .unwrap();
    file_service
        .track_file(session.id, PathBuf::from("/test/file2.txt"), None)
        .await
        .unwrap();

    let with_content = file_service
        .get_files_with_content(session.id)
        .await
        .unwrap();
    let without_content = file_service
        .get_files_without_content(session.id)
        .await
        .unwrap();

    assert_eq!(with_content.len(), 1);
    assert_eq!(without_content.len(), 1);
}
