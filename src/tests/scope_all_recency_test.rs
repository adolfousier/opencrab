//! Regression tests for #1367: scope-all model writes (`/models X all`,
//! `force_default` reload push) must change ONLY the provider/model columns.
//!
//! The pre-fix implementation looped every non-archived row through
//! `update_session`, which unconditionally restamps `updated_at = now()`.
//! One scope-all run flattened `/sessions` recency ordering (all rows
//! shared one timestamp) and made `updated_at DESC LIMIT 1` lookups
//! (title-suffix resolution, most-recent session) pick arbitrary rows.
//!
//! The fix routes both writers through a single bulk UPDATE that never
//! touches `updated_at`. These tests pin that property end-to-end against
//! a real in-memory SQLite database: timestamps seeded at whole-second
//! granularity (storage resolution) must survive byte-identical.

use crate::config::{Config, ProviderConfig, ProviderConfigs};
use crate::db::Database;
use crate::db::models::Session;
use crate::db::repository::SessionRepository;
use crate::services::force_default::apply_force_default;
use crate::services::{ServiceContext, SessionService};
use chrono::{DateTime, Utc};

fn config_with_minimax(force: bool) -> Config {
    Config {
        providers: ProviderConfigs {
            minimax: Some(ProviderConfig {
                enabled: true,
                api_key: Some("key".into()),
                default_model: Some("MiniMax-M3".into()),
                force_default: force,
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    }
}

async fn fresh_service() -> (Database, SessionService) {
    let db = Database::connect_in_memory().await.expect("in-memory DB");
    db.run_migrations().await.expect("migrations");
    let svc = SessionService::new(ServiceContext::new(db.pool().clone()));
    // Database holds the only strong pool handle; keep it alive.
    (db, svc)
}

/// Seed `n` sessions with DISTINCT provider/model pairs and staggered,
/// whole-second `updated_at` values (storage is second-resolution; seeding
/// at whole seconds makes readback equality exact).
async fn seed_staggered(db: &Database, n: usize) -> Vec<(Session, DateTime<Utc>)> {
    let repo = SessionRepository::new(db.pool().clone());
    let base = Utc::now().timestamp();
    let mut seeded = Vec::with_capacity(n);
    for i in 0..n {
        let mut s = Session::new(
            Some(format!("scope-all recency #{i}")),
            Some(format!("prov-{i}")),
            Some(format!("model-{i}")),
        );
        let ts = DateTime::from_timestamp(base - i as i64, 0).expect("valid timestamp");
        s.updated_at = ts;
        repo.create(&s).await.expect("seed session");
        seeded.push((s, ts));
    }
    seeded
}

// ── core pin: pairs change, recency does not ─────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn scope_all_changes_pairs_but_never_touches_updated_at() {
    let (_db, svc) = fresh_service().await;
    let seeded = seed_staggered(&_db, 5).await;

    let changed = svc
        .set_provider_model_all_sessions("minimax", "MiniMax-M3")
        .await
        .expect("scope-all run");
    assert_eq!(changed, 5, "every seeded row is off-pair and must change");

    for (original, seeded_ts) in &seeded {
        let after = svc
            .get_session(original.id)
            .await
            .expect("readback")
            .expect("row present");
        assert_eq!(
            after.provider_name.as_deref(),
            Some("minimax"),
            "pair must switch (#468 scope-all semantics)"
        );
        assert_eq!(after.model.as_deref(), Some("MiniMax-M3"));
        assert_eq!(
            after.updated_at, *seeded_ts,
            "#1367: scope-all must NOT restamp updated_at — recency ordering is user-visible state"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn scope_all_is_idempotent_and_never_stamps_on_noop_run() {
    let (_db, svc) = fresh_service().await;
    let seeded = seed_staggered(&_db, 3).await;

    svc.set_provider_model_all_sessions("minimax", "MiniMax-M3")
        .await
        .expect("first run");

    // Second run: every row is already on the pair — zero changes, and
    // the noop UPDATE must not silently restamp anything.
    let changed = svc
        .set_provider_model_all_sessions("minimax", "MiniMax-M3")
        .await
        .expect("second run");
    assert_eq!(changed, 0, "all rows on-pair: nothing to do");

    for (original, seeded_ts) in &seeded {
        let after = svc.get_session(original.id).await.unwrap().unwrap();
        assert_eq!(after.updated_at, *seeded_ts, "noop run must not stamp");
    }
}

// ── NULL-pair and archived-row edges ─────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn scope_all_writes_rows_with_null_pairs() {
    // Session::new(title, None, None) leaves provider_name/model NULL.
    // SQLite's `NULL IS NOT 'x'` evaluates to true, so NULL rows are
    // legitimately off-pair and must be updated (pre-fix loop did too —
    // the predicate must not regress that).
    let (_db, svc) = fresh_service().await;
    let repo = SessionRepository::new(_db.pool().clone());
    let mut null_pair = Session::new(Some("null pair".into()), None, None);
    let ts = DateTime::from_timestamp(Utc::now().timestamp(), 0).unwrap();
    null_pair.updated_at = ts;
    repo.create(&null_pair).await.expect("seed");

    let changed = svc
        .set_provider_model_all_sessions("minimax", "MiniMax-M3")
        .await
        .expect("scope-all");
    assert_eq!(changed, 1, "NULL pair row is off-pair");

    let after = svc.get_session(null_pair.id).await.unwrap().unwrap();
    assert_eq!(after.provider_name.as_deref(), Some("minimax"));
    assert_eq!(after.model.as_deref(), Some("MiniMax-M3"));
    assert_eq!(
        after.updated_at, ts,
        "#1367: even updated NULL rows keep recency"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn scope_all_excludes_archived_rows() {
    let (_db, svc) = fresh_service().await;
    let repo = SessionRepository::new(_db.pool().clone());
    // NOTE: Session::new(title, model, provider_name) — arg order matters.
    let mut live = Session::new(Some("live".into()), Some("b".into()), Some("a".into()));
    let mut archived = Session::new(Some("archived".into()), Some("b".into()), Some("a".into()));
    let base = Utc::now().timestamp();
    live.updated_at = DateTime::from_timestamp(base, 0).unwrap();
    archived.updated_at = DateTime::from_timestamp(base - 1, 0).unwrap();
    repo.create(&live).await.expect("seed live");
    repo.create(&archived).await.expect("seed archived");
    repo.archive(archived.id).await.expect("archive");

    let changed = svc
        .set_provider_model_all_sessions("minimax", "MiniMax-M3")
        .await
        .expect("scope-all");
    assert_eq!(changed, 1, "archived row excluded");

    // NOTE: archive() itself legitimately restamps updated_at (it writes
    // archived_at + updated_at together). The #1367 property under test is
    // that the BULK UPDATE doesn't touch the archived row — so we snapshot
    // the post-archive timestamp and assert the bulk preserves exactly it.
    let archived_mid = svc.get_session(archived.id).await.unwrap().unwrap();
    let archived_ts_after_archive = archived_mid.updated_at;

    let changed2 = svc
        .set_provider_model_all_sessions("minimax", "MiniMax-M3")
        .await
        .expect("scope-all over archived row");
    assert_eq!(changed2, 0, "archived row still excluded on rerun");

    let archived_after = svc.get_session(archived.id).await.unwrap().unwrap();
    assert_eq!(
        archived_after.provider_name.as_deref(),
        Some("a"),
        "archived pair untouched"
    );
    assert_eq!(
        archived_after.updated_at, archived_ts_after_archive,
        "#1367: the bulk UPDATE must not touch the archived row's recency"
    );
    assert!(
        archived_after.archived_at.is_some(),
        "archived row stays archived"
    );
}

// ── force-default reload push: same recency guarantee ────────────────

#[tokio::test(flavor = "multi_thread")]
async fn force_default_push_preserves_recency() {
    let (_db, svc) = fresh_service().await;
    let seeded = seed_staggered(&_db, 3).await;

    let changed = apply_force_default(&config_with_minimax(true), &svc)
        .await
        .expect("force-default push");
    assert_eq!(changed, 3);

    for (original, seeded_ts) in &seeded {
        let after = svc.get_session(original.id).await.unwrap().unwrap();
        assert_eq!(after.provider_name.as_deref(), Some("minimax"));
        assert_eq!(after.model.as_deref(), Some("MiniMax-M3"));
        assert_eq!(
            after.updated_at, *seeded_ts,
            "#1367: reload push must not flatten /sessions recency ordering"
        );
    }
}
