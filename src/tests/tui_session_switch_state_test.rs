//! Switching sessions must not lie about whether a turn is still running,
//! and must not lose what that turn has already produced.
//!
//! Both failures were reported together on the same switch (#1420, #1421).

use std::sync::Arc;

use uuid::Uuid;

use crate::brain::agent::service::AgentService;
use crate::brain::provider::Provider;
use crate::db::Database;
use crate::services::ServiceContext;
use crate::tests::agent_service_mocks::MockProvider;
use crate::tui::app::App;

async fn app() -> App {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let context = ServiceContext::new(db.pool().clone());
    let provider: Arc<dyn Provider> = Arc::new(MockProvider);
    let service = Arc::new(AgentService::new_for_test(provider, context.clone()).await);
    #[cfg(feature = "whatsapp")]
    {
        App::new(
            service,
            context,
            Arc::new(crate::channels::whatsapp::WhatsAppState::new()),
        )
    }
    #[cfg(not(feature = "whatsapp"))]
    {
        App::new(service, context)
    }
}

/// The regression from #1342's indicator guard: `promote_to_foreground` used
/// to restore `is_processing` from the sidecar snapshot, which is taken at
/// demote time and goes stale. A stale `false` clobbered the correct value and
/// left a running session looking idle, so its thinking indicator vanished.
#[tokio::test]
async fn promoting_a_running_session_keeps_it_marked_processing() {
    let mut app = app().await;
    let sid = Uuid::new_v4();

    // The session is running: the authoritative set says so.
    app.processing_sessions.insert(sid);

    // ...but the sidecar carries a stale snapshot claiming it is idle.
    app.is_processing = false;
    app.streaming_reasoning = Some("weighing the options".to_string());
    app.demote_to_background(sid);

    app.is_processing = false;
    assert!(app.promote_to_foreground(sid), "sidecar entry should exist");

    assert!(
        app.is_processing,
        "a session in processing_sessions must stay marked processing after promotion; \
         restoring the snapshot instead is what hid the indicator"
    );
}

/// The other direction: promotion must not invent work that is not running.
#[tokio::test]
async fn promoting_an_idle_session_leaves_it_idle() {
    let mut app = app().await;
    let sid = Uuid::new_v4();

    // Live state, but the session is NOT in the processing set.
    app.processing_sessions.insert(sid);
    app.is_processing = true;
    app.streaming_reasoning = Some("mid thought".to_string());
    app.demote_to_background(sid);
    app.processing_sessions.remove(&sid);

    app.is_processing = true;
    assert!(app.promote_to_foreground(sid));

    assert!(
        !app.is_processing,
        "a session absent from processing_sessions must not come back marked as running"
    );
}

/// Everything else in the sidecar still round-trips; only the flag changed
/// its source of truth.
#[tokio::test]
async fn the_other_live_fields_still_survive_the_round_trip() {
    let mut app = app().await;
    let sid = Uuid::new_v4();

    app.processing_sessions.insert(sid);
    app.streaming_response = Some("partial answer".to_string());
    app.streaming_reasoning = Some("thinking out loud".to_string());
    app.streaming_output_tokens = 42;
    app.demote_to_background(sid);

    app.streaming_response = None;
    app.streaming_reasoning = None;
    app.streaming_output_tokens = 0;

    assert!(app.promote_to_foreground(sid));
    assert_eq!(app.streaming_response.as_deref(), Some("partial answer"));
    assert_eq!(
        app.streaming_reasoning.as_deref(),
        Some("thinking out loud")
    );
    assert_eq!(app.streaming_output_tokens, 42);
}

/// A session with nothing live leaves no sidecar entry, so the map stays
/// bounded across switches that had no turn in flight.
#[tokio::test]
async fn an_idle_session_leaves_no_sidecar_entry() {
    let mut app = app().await;
    let sid = Uuid::new_v4();
    app.demote_to_background(sid);
    assert!(
        !app.promote_to_foreground(sid),
        "nothing live means nothing to promote"
    );
}
