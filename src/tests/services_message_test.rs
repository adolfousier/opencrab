use super::*;
use crate::services::SessionService;

async fn create_test_service() -> (MessageService, SessionService) {
    use crate::db::Database;

    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let pool = db.pool().clone();

    let context = ServiceContext::new(pool);
    (
        MessageService::new(context.clone()),
        SessionService::new(context),
    )
}

#[tokio::test]
async fn test_create_message() {
    let (message_service, session_service) = create_test_service().await;
    let session = session_service
        .create_session(Some("Test".to_string()))
        .await
        .unwrap();

    let message = message_service
        .create_message(session.id, "user".to_string(), "Hello".to_string())
        .await
        .unwrap();

    assert_eq!(message.session_id, session.id);
    assert_eq!(message.role, "user");
    assert_eq!(message.content, "Hello");
    assert_eq!(message.sequence, 1);
}

#[tokio::test]
async fn test_get_message() {
    let (message_service, session_service) = create_test_service().await;
    let session = session_service
        .create_session(Some("Test".to_string()))
        .await
        .unwrap();

    let created = message_service
        .create_message(session.id, "user".to_string(), "Test".to_string())
        .await
        .unwrap();

    let found = message_service.get_message(created.id).await.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().id, created.id);
}

#[tokio::test]
async fn test_list_messages_for_session() {
    let (message_service, session_service) = create_test_service().await;
    let session = session_service
        .create_session(Some("Test".to_string()))
        .await
        .unwrap();

    message_service
        .create_message(session.id, "user".to_string(), "Message 1".to_string())
        .await
        .unwrap();
    message_service
        .create_message(session.id, "assistant".to_string(), "Message 2".to_string())
        .await
        .unwrap();

    let messages = message_service
        .list_messages_for_session(session.id)
        .await
        .unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].sequence, 1);
    assert_eq!(messages[1].sequence, 2);
}

#[tokio::test]
async fn test_update_message_usage() {
    let (message_service, session_service) = create_test_service().await;
    let session = session_service
        .create_session(Some("Test".to_string()))
        .await
        .unwrap();

    let message = message_service
        .create_message(session.id, "user".to_string(), "Test".to_string())
        .await
        .unwrap();

    message_service
        .update_message_usage(message.id, 100, 0.05, None, None, None)
        .await
        .unwrap();

    let updated = message_service
        .get_message_required(message.id)
        .await
        .unwrap();
    assert_eq!(updated.token_count, Some(100));
    assert_eq!(updated.cost, Some(0.05));
}

#[tokio::test]
async fn test_delete_message() {
    let (message_service, session_service) = create_test_service().await;
    let session = session_service
        .create_session(Some("Test".to_string()))
        .await
        .unwrap();

    let message = message_service
        .create_message(session.id, "user".to_string(), "Test".to_string())
        .await
        .unwrap();

    message_service.delete_message(message.id).await.unwrap();

    let result = message_service.get_message(message.id).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_delete_messages_for_session() {
    let (message_service, session_service) = create_test_service().await;
    let session = session_service
        .create_session(Some("Test".to_string()))
        .await
        .unwrap();

    message_service
        .create_message(session.id, "user".to_string(), "Message 1".to_string())
        .await
        .unwrap();
    message_service
        .create_message(session.id, "assistant".to_string(), "Message 2".to_string())
        .await
        .unwrap();

    message_service
        .delete_messages_for_session(session.id)
        .await
        .unwrap();

    let messages = message_service
        .list_messages_for_session(session.id)
        .await
        .unwrap();
    assert_eq!(messages.len(), 0);
}

#[tokio::test]
async fn test_count_messages_in_session() {
    let (message_service, session_service) = create_test_service().await;
    let session = session_service
        .create_session(Some("Test".to_string()))
        .await
        .unwrap();

    message_service
        .create_message(session.id, "user".to_string(), "Message 1".to_string())
        .await
        .unwrap();
    message_service
        .create_message(session.id, "assistant".to_string(), "Message 2".to_string())
        .await
        .unwrap();

    let count = message_service
        .count_messages_in_session(session.id)
        .await
        .unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn test_get_last_message() {
    let (message_service, session_service) = create_test_service().await;
    let session = session_service
        .create_session(Some("Test".to_string()))
        .await
        .unwrap();

    message_service
        .create_message(session.id, "user".to_string(), "First".to_string())
        .await
        .unwrap();
    let last = message_service
        .create_message(session.id, "assistant".to_string(), "Last".to_string())
        .await
        .unwrap();

    let result = message_service.get_last_message(session.id).await.unwrap();
    assert!(result.is_some());
    assert_eq!(result.unwrap().id, last.id);
}

#[tokio::test]
async fn test_get_messages_by_role() {
    let (message_service, session_service) = create_test_service().await;
    let session = session_service
        .create_session(Some("Test".to_string()))
        .await
        .unwrap();

    message_service
        .create_message(session.id, "user".to_string(), "User 1".to_string())
        .await
        .unwrap();
    message_service
        .create_message(
            session.id,
            "assistant".to_string(),
            "Assistant 1".to_string(),
        )
        .await
        .unwrap();
    message_service
        .create_message(session.id, "user".to_string(), "User 2".to_string())
        .await
        .unwrap();

    let user_messages = message_service
        .get_messages_by_role(session.id, "user")
        .await
        .unwrap();
    assert_eq!(user_messages.len(), 2);

    let assistant_messages = message_service
        .get_messages_by_role(session.id, "assistant")
        .await
        .unwrap();
    assert_eq!(assistant_messages.len(), 1);
}

#[tokio::test]
async fn test_calculate_totals() {
    let (message_service, session_service) = create_test_service().await;
    let session = session_service
        .create_session(Some("Test".to_string()))
        .await
        .unwrap();

    let msg1 = message_service
        .create_message(session.id, "user".to_string(), "Message 1".to_string())
        .await
        .unwrap();
    message_service
        .update_message_usage(msg1.id, 100, 0.05, None, None, None)
        .await
        .unwrap();

    let msg2 = message_service
        .create_message(session.id, "assistant".to_string(), "Message 2".to_string())
        .await
        .unwrap();
    message_service
        .update_message_usage(msg2.id, 200, 0.10, None, None, None)
        .await
        .unwrap();

    let total_tokens = message_service
        .calculate_total_tokens(session.id)
        .await
        .unwrap();
    let total_cost = message_service
        .calculate_total_cost(session.id)
        .await
        .unwrap();

    assert_eq!(total_tokens, 300);
    assert!((total_cost - 0.15).abs() < 0.0001);
}
