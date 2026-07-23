//! #722 phase 1: the symmetric enqueue plumbing. A background-task watcher
//! resumes a session by pushing a QueuedUserMessage into the surface's queue via
//! the enqueue callback; the tool loop drains it at the next iteration boundary.
//! These lock the AgentService side of that producer.

use crate::brain::agent::service::{AgentService, MessageEnqueueCallback, QueuedUserMessage};
use crate::brain::provider::Provider;
use crate::db::Database;
use crate::services::ServiceContext;
use crate::tests::agent_service_mocks::MockProvider;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

async fn make_service() -> AgentService {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let context = ServiceContext::new(db.pool().clone());
    let provider: Arc<dyn Provider> = Arc::new(MockProvider);
    AgentService::new_for_test(provider, context).await
}

#[tokio::test]
async fn enqueue_forwards_to_the_surface_callback() {
    #[allow(clippy::type_complexity)]
    let recorded: Arc<Mutex<Vec<(Uuid, String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let rec = recorded.clone();
    let cb: MessageEnqueueCallback = Arc::new(move |sid, msg: QueuedUserMessage| {
        rec.lock()
            .unwrap()
            .push((sid, msg.context_text, msg.display_text));
    });

    let svc = make_service().await.with_message_enqueue_callback(Some(cb));
    let sid = Uuid::new_v4();

    let enqueued = svc.enqueue_session_message(
        sid,
        QueuedUserMessage::system(
            "full context for the LLM".into(),
            "[background: done]".into(),
        ),
    );

    assert!(enqueued);
    let r = recorded.lock().unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].0, sid);
    assert_eq!(r[0].1, "full context for the LLM");
    assert_eq!(r[0].2, "[background: done]");
}

#[tokio::test]
async fn enqueue_is_a_noop_without_a_callback() {
    let svc = make_service().await; // no enqueue callback wired
    let enqueued =
        svc.enqueue_session_message(Uuid::new_v4(), QueuedUserMessage::plain("x".into()));
    assert!(!enqueued);
}
