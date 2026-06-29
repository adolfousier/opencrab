use super::*;
use crate::db::models::Session;
use uuid::Uuid;

#[test]
fn test_session_new() {
    let session = Session::new(
        Some("Test Session".to_string()),
        Some("claude-sonnet-4-5".to_string()),
        Some("anthropic".to_string()),
    );

    assert_eq!(session.title, Some("Test Session".to_string()));
    assert_eq!(session.model, Some("claude-sonnet-4-5".to_string()));
    assert_eq!(session.token_count, 0);
    assert!(!session.is_archived());
}

#[test]
fn test_message_new() {
    let session_id = Uuid::new_v4();
    let message = Message::new(session_id, "user".to_string(), "Hello!".to_string(), 1);

    assert_eq!(message.session_id, session_id);
    assert_eq!(message.role, "user");
    assert_eq!(message.content, "Hello!");
    assert_eq!(message.sequence, 1);
    assert!(message.token_count.is_none());
}

#[test]
fn test_file_new() {
    let session_id = Uuid::new_v4();
    let path = std::path::PathBuf::from("/path/to/file.rs");
    let file = File::new(session_id, path.clone(), None);

    assert_eq!(file.session_id, session_id);
    assert_eq!(file.path, path);
    assert!(file.content.is_none());
}

#[test]
fn test_feedback_entry_from_row_fields() {
    // Covered by integration tests in feedback_ledger_test.rs;
    // this verifies struct construction.
    let entry = FeedbackEntry {
        id: 1,
        session_id: "abc".to_string(),
        event_type: "tool_success".to_string(),
        dimension: "bash".to_string(),
        value: 1.0,
        metadata: None,
        created_at: Utc::now(),
    };
    assert_eq!(entry.event_type, "tool_success");
    assert_eq!(entry.dimension, "bash");
    assert!((entry.value - 1.0).abs() < f64::EPSILON);
}

#[test]
fn test_session_archived() {
    let mut session = Session::new(Some("Test".to_string()), Some("model".to_string()), None);

    assert!(!session.is_archived());

    session.archived_at = Some(Utc::now());
    assert!(session.is_archived());
}
