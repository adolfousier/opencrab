//! Tests for the analytics query service + report rendering (#899).
//!
//! Drives the public `analytics_service` query functions (the layer the TUI
//! and report tool consume) against seeded in-memory data, verifies the
//! `TimeWindow` D/W/M/All filtering boundaries, and asserts the mission
//! control report renders the new phantom / per-model sections.

use crate::brain::mission_control::analytics_service;
use crate::brain::mission_control::types::{
    McAnalytics, McModelToolStat, McPhantomStats, TimeWindow,
};
use crate::brain::tools::mission_control_report::render_markdown;
use crate::db::Database;
use crate::db::Pool;
use crate::db::repository::AnalyticsEventRepository;

/// Helper: in-memory DB with migrations applied.
async fn test_pool() -> Pool {
    let db = Database::connect_in_memory()
        .await
        .expect("in-memory DB should connect");
    db.run_migrations().await.expect("migrations should apply");
    db.pool().clone()
}

/// Insert a phantom event with an explicit `detected_at` epoch (the repo
/// methods always stamp "now", so raw SQL is needed to test time windows).
async fn seed_phantom_at(pool: &Pool, id: &str, model: &str, detected_at: i64, resolved: bool) {
    let id = id.to_string();
    let model = model.to_string();
    let conn = pool.get().await.expect("should get connection");
    conn.interact(move |conn| {
        conn.execute(
            "INSERT INTO phantom_events (id, session_id, provider, model, detected_at, resolved) \
             VALUES (?1, 'sess', 'anthropic', ?2, ?3, ?4)",
            rusqlite::params![id, model, detected_at, resolved as i64],
        )
    })
    .await
    .expect("interact should succeed")
    .expect("insert should succeed");
}

// ─── phantom_stats: totals + resolution ratio ─────────────────────────

#[tokio::test]
async fn phantom_stats_totals_and_ratio() {
    let pool = test_pool().await;
    let now = chrono::Utc::now().timestamp();

    // 4 phantoms, 3 resolved -> 75% resolved. Seed resolved states directly so
    // the ratio math is tested independently of resolve_phantom's timestamp
    // semantics (it resolves every unresolved row sharing the max detected_at).
    seed_phantom_at(&pool, "ph-0", "claude-opus-4-8", now, true).await;
    seed_phantom_at(&pool, "ph-1", "claude-opus-4-8", now, true).await;
    seed_phantom_at(&pool, "ph-2", "claude-opus-4-8", now, true).await;
    seed_phantom_at(&pool, "ph-3", "claude-opus-4-8", now, false).await;

    let stats = analytics_service::phantom_stats(pool, TimeWindow::All).await;
    assert_eq!(stats.total, 4);
    assert_eq!(stats.resolved, 3);
    assert_eq!(stats.resolved_pct, 75.0);
}

#[tokio::test]
async fn phantom_stats_zero_total_has_zero_ratio() {
    let pool = test_pool().await;
    let stats = analytics_service::phantom_stats(pool, TimeWindow::All).await;
    assert_eq!(stats.total, 0);
    assert_eq!(stats.resolved_pct, 0.0);
    assert!(stats.by_model.is_empty());
}

// ─── phantom_stats: per-model breakdown ───────────────────────────────

#[tokio::test]
async fn phantom_stats_per_model_breakdown() {
    let pool = test_pool().await;
    let repo = AnalyticsEventRepository::new(pool.clone());

    repo.record_phantom("a", "s1", Some("anthropic"), Some("claude-opus-4-8"))
        .await
        .unwrap();
    repo.record_phantom("b", "s2", Some("anthropic"), Some("claude-opus-4-8"))
        .await
        .unwrap();
    repo.record_phantom("c", "s3", Some("modelstudio"), Some("qwen3.8-max-preview"))
        .await
        .unwrap();

    let stats = analytics_service::phantom_stats(pool, TimeWindow::All).await;
    assert_eq!(stats.total, 3);
    assert_eq!(stats.by_model.len(), 2);
    // Most phantoms first: claude-opus-4-8 (2) before qwen (1).
    assert_eq!(stats.by_model[0].0, "claude-opus-4-8");
    assert_eq!(stats.by_model[0].1, 2);
    assert_eq!(stats.by_model[1].0, "qwen3.8-max-preview");
    assert_eq!(stats.by_model[1].1, 1);
}

// ─── TimeWindow filtering boundaries (D/W/M/All) ──────────────────────

#[tokio::test]
async fn phantom_stats_time_window_filtering() {
    let pool = test_pool().await;
    let now = chrono::Utc::now().timestamp();

    // 40 days ago: only All sees it.
    seed_phantom_at(&pool, "old", "claude-opus-4-8", now - 40 * 86_400, false).await;
    // 2 days ago: Week/Month/All see it, Day does not.
    seed_phantom_at(&pool, "recent", "claude-opus-4-8", now - 2 * 86_400, false).await;

    let all = analytics_service::phantom_stats(pool.clone(), TimeWindow::All).await;
    let month = analytics_service::phantom_stats(pool.clone(), TimeWindow::Month).await;
    let week = analytics_service::phantom_stats(pool.clone(), TimeWindow::Week).await;
    let day = analytics_service::phantom_stats(pool.clone(), TimeWindow::Day).await;

    assert_eq!(all.total, 2, "All should see both events");
    assert_eq!(
        month.total, 1,
        "Month (30d) should exclude the 40d-old event"
    );
    assert_eq!(week.total, 1, "Week (7d) should include the 2d-old event");
    assert_eq!(day.total, 0, "Day (1d) should exclude the 2d-old event");
}

#[tokio::test]
async fn time_window_since_epoch_boundaries() {
    let now = chrono::Utc::now().timestamp();
    assert_eq!(TimeWindow::All.since_epoch(), None);
    // Each bounded window is a positive epoch in the past, ordered Day > Week > Month.
    let d = TimeWindow::Day.since_epoch().unwrap();
    let w = TimeWindow::Week.since_epoch().unwrap();
    let m = TimeWindow::Month.since_epoch().unwrap();
    assert!(d > w && w > m, "Day bound is most recent, Month least");
    assert!(m < now && d < now);
}

// ─── brain_verify_stats ───────────────────────────────────────────────

#[tokio::test]
async fn brain_verify_stats_counts_per_outcome() {
    let pool = test_pool().await;
    let repo = AnalyticsEventRepository::new(pool.clone());

    for i in 0..3 {
        repo.record_brain_verify(&format!("p{i}"), "SOUL.md", "pass", None)
            .await
            .unwrap();
    }
    for i in 0..2 {
        repo.record_brain_verify(&format!("r{i}"), "MEMORY.md", "rollback", Some("x"))
            .await
            .unwrap();
    }
    repo.record_brain_verify("f0", "AGENTS.md", "fail_closed", Some("y"))
        .await
        .unwrap();

    let stats = analytics_service::brain_verify_stats(pool, TimeWindow::All).await;
    assert_eq!(stats.passes, 3);
    assert_eq!(stats.rollbacks, 2);
    assert_eq!(stats.fail_closed, 1);
}

// ─── streaming_stats + tool_stats_by_model ────────────────────────────

#[tokio::test]
async fn streaming_and_model_stats() {
    let pool = test_pool().await;
    let repo = AnalyticsEventRepository::new(pool.clone());

    repo.record_streaming_recovery("sr1", "s", Some("anthropic"), Some("claude-opus-4-8"), 3)
        .await
        .unwrap();
    let streaming = analytics_service::streaming_stats(pool.clone(), TimeWindow::All).await;
    assert_eq!(streaming.total, 1);
    assert_eq!(streaming.total_tools, 3);
    assert_eq!(streaming.by_model[0].0, "claude-opus-4-8");

    // Seed tool_executions with provider/model to drive the per-model breakdown.
    {
        let conn = pool.get().await.expect("conn");
        conn.interact(|conn| {
            conn.execute_batch(
                "INSERT INTO tool_executions (id, message_id, session_id, tool_name, status, created_at, provider, model) \
                 VALUES ('t1','m','s','bash','success', strftime('%s','now'), 'anthropic','claude-opus-4-8'); \
                 INSERT INTO tool_executions (id, message_id, session_id, tool_name, status, created_at, provider, model) \
                 VALUES ('t2','m','s','bash','error', strftime('%s','now'), 'anthropic','claude-opus-4-8');",
            )
        })
        .await
        .expect("interact")
        .expect("insert");
    }
    let models = analytics_service::tool_stats_by_model(pool, TimeWindow::All).await;
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].model, "claude-opus-4-8");
    assert_eq!(models[0].total, 2);
    assert_eq!(models[0].failures, 1);
    assert_eq!(models[0].fail_rate, 50.0);
}

// ─── mission_control_report renders the new sections ──────────────────

#[test]
fn report_renders_phantom_and_model_sections() {
    let a = McAnalytics {
        phantom: McPhantomStats {
            total: 5,
            resolved: 4,
            resolved_pct: 80.0,
            by_model: vec![("claude-opus-4-8".into(), 5, 4)],
        },
        model_tools: vec![McModelToolStat {
            model: "claude-opus-4-8".into(),
            total: 100,
            failures: 7,
            fail_rate: 7.0,
        }],
        ..Default::default()
    };

    let md = render_markdown(&a, &[], &[], &[]);
    assert!(md.contains("Phantoms"), "metric row present");
    assert!(md.contains("5 detected, 4 resolved (80.0%)"));
    assert!(
        md.contains("Phantom by Model"),
        "per-model phantom table present"
    );
    assert!(
        md.contains("Tool Reliability by Model"),
        "per-model reliability table present"
    );
    assert!(md.contains("claude-opus-4-8"));
    assert!(!md.contains('\u{2014}'), "report must stay em-dash-free");
}

#[test]
fn report_omits_empty_model_sections() {
    // With no phantom/model data the per-model tables should not render.
    let a = McAnalytics::default();
    let md = render_markdown(&a, &[], &[], &[]);
    assert!(!md.contains("Phantom by Model"));
    assert!(!md.contains("Tool Reliability by Model"));
}
