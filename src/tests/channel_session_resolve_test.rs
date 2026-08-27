//! Tests for the shared channel session resolver (Discord/Slack/WhatsApp port
//! of the Telegram suffix-lookup pattern). Issue #121.

use crate::channels::session_resolve::{
    chat_id_suffix, resolve_or_create_channel_session, session_idle_expired,
};
use crate::db::Database;
use crate::services::{ServiceContext, SessionService};

async fn fresh_session_service() -> SessionService {
    let db = Database::connect_in_memory()
        .await
        .expect("in-memory db connect");
    db.run_migrations().await.expect("run migrations");
    SessionService::new(ServiceContext::new(db.pool().clone()))
}

#[test]
fn chat_id_suffix_format() {
    assert_eq!(chat_id_suffix("discord-12345"), "[chat:discord-12345]");
    assert_eq!(chat_id_suffix("wa-+15551234567"), "[chat:wa-+15551234567]");
    assert_eq!(chat_id_suffix("slack-dm-U0ABC"), "[chat:slack-dm-U0ABC]");
}

#[test]
fn idle_window_logic_matches_telegram_helper() {
    let recent = chrono::Utc::now() - chrono::Duration::minutes(30);
    let stale = chrono::Utc::now() - chrono::Duration::hours(2);

    assert!(!session_idle_expired(recent, Some(1.0)));
    assert!(session_idle_expired(stale, Some(1.0)));
    // Disabled idle window: never expired.
    assert!(!session_idle_expired(stale, None));
}

#[tokio::test]
async fn resolves_existing_session_by_suffix() {
    let svc = fresh_session_service().await;
    let suffix = chat_id_suffix("discord-1");
    let legacy = "Discord: #1".to_string();
    let title = format!("{legacy} {suffix}");

    let created = svc
        .create_session(Some(title.clone()))
        .await
        .expect("create");

    let resolved =
        resolve_or_create_channel_session(&svc, &suffix, &legacy, &title, None, "Discord")
            .await
            .expect("resolve");
    assert_eq!(resolved, created.id);
}

#[tokio::test]
async fn suffix_lookup_survives_auto_rename() {
    let svc = fresh_session_service().await;
    let suffix = chat_id_suffix("slack-C0123");
    let legacy = "Slack: #C0123".to_string();
    let title = format!("{legacy} {suffix}");

    let created = svc
        .create_session(Some(title.clone()))
        .await
        .expect("create");

    // Simulate auto-title rewriting the visible label but preserving the suffix.
    let mut renamed = created.clone();
    renamed.title = Some(format!("Deploy planning {suffix}"));
    svc.update_session(&renamed).await.expect("rename");

    let resolved = resolve_or_create_channel_session(&svc, &suffix, &legacy, &title, None, "Slack")
        .await
        .expect("resolve");
    assert_eq!(
        resolved, created.id,
        "auto-rename must not orphan the session"
    );
}

#[tokio::test]
async fn forward_migrates_legacy_pre_suffix_row() {
    let svc = fresh_session_service().await;
    let suffix = chat_id_suffix("wa-+15551234567");
    let legacy = "WhatsApp: +15551234567".to_string();
    let title = format!("{legacy} {suffix}");

    // Pre-fix row had no suffix.
    let created = svc
        .create_session(Some(legacy.clone()))
        .await
        .expect("create legacy");

    let resolved =
        resolve_or_create_channel_session(&svc, &suffix, &legacy, &title, None, "WhatsApp")
            .await
            .expect("resolve");

    assert_eq!(resolved, created.id, "must reuse legacy row, not duplicate");

    let after = svc
        .get_session(resolved)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(
        after.title.as_deref(),
        Some(title.as_str()),
        "legacy row title forward-migrated to include suffix"
    );

    // Second call resolves via suffix path now that the row was migrated.
    let resolved2 =
        resolve_or_create_channel_session(&svc, &suffix, &legacy, &title, None, "WhatsApp")
            .await
            .expect("resolve again");
    assert_eq!(resolved2, created.id);
}

#[tokio::test]
async fn creates_when_no_match_exists() {
    let svc = fresh_session_service().await;
    let suffix = chat_id_suffix("discord-dm-999");
    let legacy = "Discord: DM Alice (999)".to_string();
    let title = format!("{legacy} {suffix}");

    let resolved =
        resolve_or_create_channel_session(&svc, &suffix, &legacy, &title, None, "Discord")
            .await
            .expect("resolve creates");

    let row = svc
        .get_session(resolved)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(row.title.as_deref(), Some(title.as_str()));
    assert!(!row.is_archived());
}

#[tokio::test]
async fn idle_session_archives_and_creates_new() {
    use rusqlite::params;
    let svc = fresh_session_service().await;
    let suffix = chat_id_suffix("discord-2");
    let legacy = "Discord: #2".to_string();
    let title = format!("{legacy} {suffix}");

    let created = svc
        .create_session(Some(title.clone()))
        .await
        .expect("create");

    // Backdate updated_at via direct SQL since update_session always stamps now().
    // Scope the connection handle tight — the in-memory pool is single-conn
    // (see Database::connect_in_memory), so holding `conn` across the
    // resolve_or_create_channel_session call below would deadlock.
    {
        let conn = svc.pool().get().await.expect("conn");
        let session_id_str = created.id.to_string();
        let backdated_ts = (chrono::Utc::now() - chrono::Duration::hours(2)).timestamp();
        conn.interact(move |c| {
            c.execute(
                "UPDATE sessions SET updated_at = ?1 WHERE id = ?2",
                params![backdated_ts, session_id_str],
            )
        })
        .await
        .expect("interact")
        .expect("backdate");
    }

    let resolved = resolve_or_create_channel_session(
        &svc,
        &suffix,
        &legacy,
        &title,
        Some(1.0), // 1h idle window
        "Discord",
    )
    .await
    .expect("resolve idle");

    assert_ne!(resolved, created.id, "idle reset must produce a new row");

    let archived = svc
        .get_session(created.id)
        .await
        .expect("get old")
        .expect("exists");
    assert!(archived.is_archived(), "old row must be archived");
}

// ---------------------------------------------------------------------------
// Single-flight (#1201 generalized, #1228): the shared gate the generic
// resolver holds must serialize look-up-then-create per key, so two
// near-simultaneous messages into one fresh chat create ONE session.
// We pin the gate's semantics directly (concurrent resolve-one-create) with
// the same await-between-halves shape the DB path has, without a live DB —
// the in-memory pool is single-conn and holding it across a resolve would
// deadlock (see idle test note above).
// ---------------------------------------------------------------------------
#[tokio::test]
async fn concurrent_resolves_create_exactly_one_session() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // An in-memory stand-in for `SessionService.persistence`: Option stores
    // the created session id, atomics count creations.
    let store = Arc::new(tokio::sync::Mutex::new(None::<u64>));
    let created = Arc::new(AtomicUsize::new(0));

    // Same look-then-create shape as resolve_or_create_channel_session:
    // lookup, await, then insert only if the lookup missed.
    async fn resolve_once(
        store: &Arc<tokio::sync::Mutex<Option<u64>>>,
        created: &AtomicUsize,
    ) -> u64 {
        let existing = *store.lock().await;
        // The await (DB round-trip inside real resolution) that would let a
        // second concurrent caller in and both miss.
        tokio::task::yield_now().await;
        if let Some(id) = existing {
            id
        } else {
            created.fetch_add(1, Ordering::SeqCst);
            let id = 42u64;
            *store.lock().await = Some(id);
            id
        }
    }

    const KEY: &str = "chan:Discord:[chat:testsuite-dm-1]";

    let tasks: Vec<_> = (0..2)
        .map(|_| {
            let store = Arc::clone(&store);
            let created = Arc::clone(&created);
            tokio::spawn(async move {
                // The resolver's gate guards the whole look-up-then-create.
                let _g = crate::channels::single_flight::hold(KEY).await;
                resolve_once(&store, &created).await
            })
        })
        .collect();
    let mut ids = Vec::new();
    for t in tasks {
        ids.push(t.await.expect("resolve task panicked"));
    }

    assert_eq!(
        created.load(Ordering::SeqCst),
        1,
        "#1228: the second message must find the first's session, \
         not create a parallel one"
    );
    // Both callers observed the single session.
    assert_eq!(ids, vec![42, 42]);
}

#[tokio::test]
async fn shared_gate_distinct_keys_do_not_block() {
    const KEY_A: &str = "chan:Discord:[chat:a]";
    const KEY_B: &str = "chan:Discord:[chat:b]";

    let held = crate::channels::single_flight::hold(KEY_A).await;

    let other = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        crate::channels::single_flight::hold(KEY_B),
    )
    .await;
    assert!(
        other.is_ok(),
        "#1228: a distinct chat id must not wait on another's gate"
    );
    drop(held);
}

#[tokio::test]
async fn same_key_serializes_then_releases() {
    const KEY: &str = "chan:Slack:#general";

    let held = crate::channels::single_flight::hold(KEY).await;
    let blocked = tokio::time::timeout(
        std::time::Duration::from_millis(200),
        crate::channels::single_flight::hold(KEY),
    )
    .await;
    assert!(
        blocked.is_err(),
        "#1228: the same key must be single-flight"
    );
    drop(held);

    let after = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        crate::channels::single_flight::hold(KEY),
    )
    .await;
    assert!(
        after.is_ok(),
        "#1228: the gate must be released, not leaked"
    );
}
