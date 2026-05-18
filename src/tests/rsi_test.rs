//! Recursive Self-Improvement (RSI) Tests
//!
//! Comprehensive tests for the feedback ledger repository, feedback_record tool,
//! feedback_analyze tool, and self_improve tool.

// --- Feedback Ledger Repository Tests ---

mod feedback_ledger_repo {
    use crate::db::Database;
    use crate::db::repository::FeedbackLedgerRepository;

    async fn setup() -> (Database, FeedbackLedgerRepository) {
        let db = Database::connect_in_memory().await.expect("in-memory DB");
        db.run_migrations().await.expect("migrations");
        let repo = FeedbackLedgerRepository::new(db.pool().clone());
        (db, repo)
    }

    #[tokio::test]
    async fn record_and_count() {
        let (_db, repo) = setup().await;
        assert_eq!(repo.total_count().await.unwrap(), 0);

        let id = repo
            .record("sess1", "tool_success", "bash", 1.0, None)
            .await
            .unwrap();
        assert!(id > 0);
        assert_eq!(repo.total_count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn record_with_metadata() {
        let (_db, repo) = setup().await;
        let id = repo
            .record(
                "sess1",
                "tool_failure",
                "edit",
                0.0,
                Some(r#"{"error":"file not found"}"#),
            )
            .await
            .unwrap();
        assert!(id > 0);

        let entries = repo.recent(10).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].event_type, "tool_failure");
        assert_eq!(entries[0].dimension, "edit");
        assert!(
            entries[0]
                .metadata
                .as_deref()
                .unwrap()
                .contains("file not found")
        );
    }

    #[tokio::test]
    async fn recent_returns_latest() {
        let (_db, repo) = setup().await;
        for i in 0..5 {
            repo.record("sess1", "tool_success", &format!("tool_{i}"), 1.0, None)
                .await
                .unwrap();
        }

        let entries = repo.recent(3).await.unwrap();
        assert_eq!(entries.len(), 3);
        // Should return some subset of the 5 entries (ordering by created_at DESC, rowid tiebreak)
        // All entries should be from our set
        for e in &entries {
            assert!(e.dimension.starts_with("tool_"));
        }
    }

    #[tokio::test]
    async fn recent_respects_limit() {
        let (_db, repo) = setup().await;
        for i in 0..10 {
            repo.record("sess1", "tool_success", &format!("t{i}"), 1.0, None)
                .await
                .unwrap();
        }
        let entries = repo.recent(5).await.unwrap();
        assert_eq!(entries.len(), 5);
    }

    #[tokio::test]
    async fn by_event_type_filters() {
        let (_db, repo) = setup().await;
        repo.record("s1", "tool_success", "bash", 1.0, None)
            .await
            .unwrap();
        repo.record("s1", "tool_failure", "edit", 0.0, None)
            .await
            .unwrap();
        repo.record("s1", "tool_success", "read", 1.0, None)
            .await
            .unwrap();
        repo.record("s1", "user_correction", "tone", 1.0, None)
            .await
            .unwrap();

        let successes = repo.by_event_type("tool_success", 50).await.unwrap();
        assert_eq!(successes.len(), 2);
        for e in &successes {
            assert_eq!(e.event_type, "tool_success");
        }

        let failures = repo.by_event_type("tool_failure", 50).await.unwrap();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].dimension, "edit");

        let corrections = repo.by_event_type("user_correction", 50).await.unwrap();
        assert_eq!(corrections.len(), 1);
    }

    #[tokio::test]
    async fn by_event_type_empty() {
        let (_db, repo) = setup().await;
        let entries = repo.by_event_type("nonexistent", 50).await.unwrap();
        assert!(entries.is_empty());
    }

    #[tokio::test]
    async fn stats_by_dimension() {
        let (_db, repo) = setup().await;
        // bash: 3 success, 1 failure
        for _ in 0..3 {
            repo.record("s1", "tool_success", "bash", 1.0, None)
                .await
                .unwrap();
        }
        repo.record("s1", "tool_failure", "bash", 0.0, None)
            .await
            .unwrap();
        // edit: 1 success, 2 failures
        repo.record("s1", "tool_success", "edit", 1.0, None)
            .await
            .unwrap();
        for _ in 0..2 {
            repo.record("s1", "tool_failure", "edit", 0.0, None)
                .await
                .unwrap();
        }

        let stats = repo.stats_by_dimension("tool_").await.unwrap();
        assert_eq!(stats.len(), 2);

        // bash has more total events, should be first
        let bash = &stats[0];
        assert_eq!(bash.dimension, "bash");
        assert_eq!(bash.total_events, 4);
        assert_eq!(bash.successes, 3);
        assert_eq!(bash.failures, 1);
        assert!((bash.success_rate - 0.75).abs() < 0.01);

        let edit = &stats[1];
        assert_eq!(edit.dimension, "edit");
        assert_eq!(edit.total_events, 3);
        assert_eq!(edit.successes, 1);
        assert_eq!(edit.failures, 2);
        assert!((edit.success_rate - 1.0 / 3.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn stats_by_dimension_empty() {
        let (_db, repo) = setup().await;
        let stats = repo.stats_by_dimension("tool_").await.unwrap();
        assert!(stats.is_empty());
    }

    #[tokio::test]
    async fn summary_groups_by_event_type() {
        let (_db, repo) = setup().await;
        repo.record("s1", "tool_success", "bash", 1.0, None)
            .await
            .unwrap();
        repo.record("s1", "tool_success", "read", 1.0, None)
            .await
            .unwrap();
        repo.record("s1", "tool_failure", "edit", 0.0, None)
            .await
            .unwrap();
        repo.record("s1", "user_correction", "tone", 1.0, None)
            .await
            .unwrap();

        let summary = repo.summary().await.unwrap();
        assert_eq!(summary.len(), 3);
        // Ordered by count DESC
        assert_eq!(summary[0].0, "tool_success");
        assert_eq!(summary[0].1, 2);
    }

    #[tokio::test]
    async fn summary_empty_ledger() {
        let (_db, repo) = setup().await;
        let summary = repo.summary().await.unwrap();
        assert!(summary.is_empty());
    }

    #[tokio::test]
    async fn count_since_filters_by_date() {
        let (_db, repo) = setup().await;
        repo.record("s1", "tool_success", "bash", 1.0, None)
            .await
            .unwrap();

        // All events should be "since" a long time ago
        let count = repo.count_since("2000-01-01T00:00:00Z").await.unwrap();
        assert_eq!(count, 1);

        // None should be "since" a future date
        let count = repo.count_since("2099-01-01T00:00:00Z").await.unwrap();
        assert_eq!(count, 0);
    }

    /// Helper: insert a feedback row at a specific historical timestamp,
    /// bypassing `record()` which always stamps "now". Required to
    /// simulate the staleness scenario the windowed stats are built for.
    async fn record_at(
        repo: &crate::db::repository::FeedbackLedgerRepository,
        session_id: &str,
        event_type: &str,
        dimension: &str,
        value: f64,
        created_at: &str,
    ) {
        let sid = session_id.to_string();
        let et = event_type.to_string();
        let dim = dimension.to_string();
        let ts = created_at.to_string();
        repo.pool()
            .get()
            .await
            .unwrap()
            .interact(move |conn| -> rusqlite::Result<()> {
                conn.execute(
                    "INSERT INTO feedback_ledger (session_id, event_type, dimension, value, created_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![sid, et, dim, value, ts],
                )?;
                Ok(())
            })
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn stats_by_dimension_since_none_matches_lifetime() {
        // Pinning that passing None to the windowed variant is identical
        // to the old (lifetime) `stats_by_dimension`. Without this the
        // two could quietly diverge on schema changes.
        let (_db, repo) = setup().await;
        for _ in 0..3 {
            repo.record("s1", "tool_success", "bash", 1.0, None)
                .await
                .unwrap();
        }
        repo.record("s1", "tool_failure", "bash", 0.0, None)
            .await
            .unwrap();

        let lifetime = repo.stats_by_dimension("tool_").await.unwrap();
        let windowed = repo.stats_by_dimension_since("tool_", None).await.unwrap();
        assert_eq!(lifetime.len(), windowed.len());
        assert_eq!(lifetime[0].dimension, windowed[0].dimension);
        assert_eq!(lifetime[0].total_events, windowed[0].total_events);
        assert_eq!(lifetime[0].successes, windowed[0].successes);
        assert_eq!(lifetime[0].failures, windowed[0].failures);
    }

    #[tokio::test]
    async fn stats_by_dimension_since_drops_stale_failures() {
        // Regression: the 2026-04-25 RSI logs reported "exa_search 100%
        // failure (5/5)" forever because old failures from 2026-04-14/17
        // never aged out. After the fix we window stats; the same
        // failures with no recent activity must report zero events
        // inside a 7-day window.
        let (_db, repo) = setup().await;
        let stale = "2026-04-14T22:49:45Z";
        for _ in 0..5 {
            record_at(&repo, "s1", "tool_failure", "exa_search", 0.0, stale).await;
        }
        // Today's reference point — picked so the stale rows fall
        // 11 days behind it.
        let since = "2026-04-25T00:00:00Z";
        let stats = repo
            .stats_by_dimension_since("tool_", Some(since))
            .await
            .unwrap();
        assert!(
            stats.iter().all(|s| s.dimension != "exa_search"),
            "exa_search must be excluded once its only events are outside the window: {:?}",
            stats
        );
    }

    #[tokio::test]
    async fn stats_by_dimension_since_keeps_recent_real_failures() {
        // Mirror image of the above: tools with failures inside the
        // window stay reported. browser_navigate had failures on
        // 2026-04-22 — those must survive a 7-day window from 04-25.
        let (_db, repo) = setup().await;
        let recent = "2026-04-22T12:54:40Z";
        let stale = "2026-04-13T00:31:08Z";
        for _ in 0..4 {
            record_at(&repo, "s1", "tool_failure", "browser_navigate", 0.0, recent).await;
        }
        // Old successes that should NOT inflate the in-window denominator.
        for _ in 0..10 {
            record_at(&repo, "s1", "tool_success", "browser_navigate", 1.0, stale).await;
        }

        let since = "2026-04-18T00:00:00Z";
        let stats = repo
            .stats_by_dimension_since("tool_", Some(since))
            .await
            .unwrap();
        let row = stats
            .iter()
            .find(|s| s.dimension == "browser_navigate")
            .expect("browser_navigate must remain visible inside the window");
        assert_eq!(row.total_events, 4, "only the 4 in-window failures count");
        assert_eq!(row.failures, 4);
        assert_eq!(row.successes, 0);
        assert!(
            row.success_rate < 0.01,
            "with all in-window events being failures, success_rate must be ~0"
        );
    }

    #[tokio::test]
    async fn stats_by_dimension_since_mixed_events_inside_window() {
        // Sanity: a tool with both successes and failures inside the
        // window reports the right ratio.
        let (_db, repo) = setup().await;
        let recent = "2026-04-24T10:00:00Z";
        for _ in 0..3 {
            record_at(&repo, "s1", "tool_success", "bash", 1.0, recent).await;
        }
        record_at(&repo, "s1", "tool_failure", "bash", 0.0, recent).await;

        let since = "2026-04-18T00:00:00Z";
        let stats = repo
            .stats_by_dimension_since("tool_", Some(since))
            .await
            .unwrap();
        let bash = stats
            .iter()
            .find(|s| s.dimension == "bash")
            .expect("bash present");
        assert_eq!(bash.total_events, 4);
        assert_eq!(bash.successes, 3);
        assert_eq!(bash.failures, 1);
        assert!((bash.success_rate - 0.75).abs() < 0.01);
    }

    #[tokio::test]
    async fn multiple_sessions() {
        let (_db, repo) = setup().await;
        repo.record("sess_a", "tool_success", "bash", 1.0, None)
            .await
            .unwrap();
        repo.record("sess_b", "tool_failure", "bash", 0.0, None)
            .await
            .unwrap();

        assert_eq!(repo.total_count().await.unwrap(), 2);
        let entries = repo.recent(10).await.unwrap();
        let sessions: Vec<&str> = entries.iter().map(|e| e.session_id.as_str()).collect();
        assert!(sessions.contains(&"sess_a"));
        assert!(sessions.contains(&"sess_b"));
    }

    #[tokio::test]
    async fn value_preserved() {
        let (_db, repo) = setup().await;
        repo.record("s1", "context_compaction", "tokens", 4096.0, None)
            .await
            .unwrap();
        let entries = repo.recent(1).await.unwrap();
        assert!((entries[0].value - 4096.0).abs() < 0.01);
    }
}

// --- Feedback Record Tool Tests ---

mod feedback_record_tool {
    use crate::brain::tools::feedback_record::FeedbackRecordTool;
    use crate::brain::tools::{Tool, ToolExecutionContext};
    use crate::db::Database;
    use crate::services::ServiceContext;
    use serde_json::json;
    use uuid::Uuid;

    async fn setup() -> (Database, ToolExecutionContext) {
        let db = Database::connect_in_memory().await.expect("in-memory DB");
        db.run_migrations().await.expect("migrations");
        let svc = ServiceContext::new(db.pool().clone());
        let mut ctx = ToolExecutionContext::new(Uuid::new_v4());
        ctx.service_context = Some(svc);
        (db, ctx)
    }

    #[test]
    fn tool_metadata() {
        let tool = FeedbackRecordTool;
        assert_eq!(tool.name(), "feedback_record");
        assert!(!tool.requires_approval());
        assert!(tool.capabilities().is_empty());
        let schema = tool.input_schema();
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "event_type"));
        assert!(required.iter().any(|v| v == "dimension"));
    }

    #[tokio::test]
    async fn record_success() {
        let (_db, ctx) = setup().await;
        let tool = FeedbackRecordTool;
        let result = tool
            .execute(
                json!({
                    "event_type": "tool_success",
                    "dimension": "bash",
                    "value": 1.0
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("Recorded feedback"));
        assert!(result.output.contains("tool_success/bash"));
    }

    #[tokio::test]
    async fn record_with_metadata() {
        let (_db, ctx) = setup().await;
        let tool = FeedbackRecordTool;
        let result = tool
            .execute(
                json!({
                    "event_type": "tool_failure",
                    "dimension": "edit",
                    "value": 0.0,
                    "metadata": "file was read-only"
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("tool_failure/edit"));
    }

    #[tokio::test]
    async fn record_default_value() {
        let (_db, ctx) = setup().await;
        let tool = FeedbackRecordTool;
        let result = tool
            .execute(
                json!({
                    "event_type": "pattern_observed",
                    "dimension": "user_prefers_concise"
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.success);
        // Default value is 1.0
        assert!(result.output.contains("= 1"));
    }

    #[tokio::test]
    async fn record_missing_event_type() {
        let (_db, ctx) = setup().await;
        let tool = FeedbackRecordTool;
        let result = tool
            .execute(
                json!({
                    "dimension": "bash"
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!result.success);
        let err = result.error.as_deref().unwrap();
        assert!(err.contains("required"));
    }

    #[tokio::test]
    async fn record_missing_dimension() {
        let (_db, ctx) = setup().await;
        let tool = FeedbackRecordTool;
        let result = tool
            .execute(
                json!({
                    "event_type": "tool_success"
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!result.success);
        let err = result.error.as_deref().unwrap();
        assert!(err.contains("required"));
    }

    #[tokio::test]
    async fn record_empty_strings() {
        let (_db, ctx) = setup().await;
        let tool = FeedbackRecordTool;
        let result = tool
            .execute(
                json!({
                    "event_type": "",
                    "dimension": ""
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!result.success);
    }

    #[tokio::test]
    async fn record_no_service_context() {
        let ctx = ToolExecutionContext::new(Uuid::new_v4());
        let tool = FeedbackRecordTool;
        let result = tool
            .execute(
                json!({
                    "event_type": "tool_success",
                    "dimension": "bash"
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!result.success);
        let err = result.error.as_deref().unwrap();
        assert!(err.contains("database"));
    }
}

// --- Feedback Analyze Tool Tests ---

mod feedback_analyze_tool {
    use crate::brain::tools::feedback_analyze::FeedbackAnalyzeTool;
    use crate::brain::tools::{Tool, ToolExecutionContext};
    use crate::db::Database;
    use crate::db::repository::FeedbackLedgerRepository;
    use crate::services::ServiceContext;
    use serde_json::json;
    use uuid::Uuid;

    async fn setup() -> (Database, ToolExecutionContext, FeedbackLedgerRepository) {
        let db = Database::connect_in_memory().await.expect("in-memory DB");
        db.run_migrations().await.expect("migrations");
        let repo = FeedbackLedgerRepository::new(db.pool().clone());
        let svc = ServiceContext::new(db.pool().clone());
        let mut ctx = ToolExecutionContext::new(Uuid::new_v4());
        ctx.service_context = Some(svc);
        (db, ctx, repo)
    }

    /// Helper to get the text from a ToolResult (output for success, error for failure)
    fn result_text(result: &crate::brain::tools::ToolResult) -> &str {
        if result.success {
            &result.output
        } else {
            result.error.as_deref().unwrap_or("")
        }
    }

    #[test]
    fn tool_metadata() {
        let tool = FeedbackAnalyzeTool;
        assert_eq!(tool.name(), "feedback_analyze");
        assert!(!tool.requires_approval());
        assert!(tool.capabilities().is_empty());
    }

    #[tokio::test]
    async fn summary_empty_ledger() {
        let (_db, ctx, _repo) = setup().await;
        let tool = FeedbackAnalyzeTool;
        let result = tool
            .execute(json!({"query": "summary"}), &ctx)
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("No feedback data yet"));
    }

    #[tokio::test]
    async fn summary_with_data() {
        let (_db, ctx, repo) = setup().await;
        repo.record("s1", "tool_success", "bash", 1.0, None)
            .await
            .unwrap();
        repo.record("s1", "tool_failure", "edit", 0.0, None)
            .await
            .unwrap();
        repo.record("s1", "tool_success", "read", 1.0, None)
            .await
            .unwrap();

        let tool = FeedbackAnalyzeTool;
        let result = tool
            .execute(json!({"query": "summary"}), &ctx)
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("3 total events"));
        assert!(result.output.contains("tool_success"));
        assert!(result.output.contains("tool_failure"));
    }

    #[tokio::test]
    async fn tool_stats_empty() {
        let (_db, ctx, _repo) = setup().await;
        let tool = FeedbackAnalyzeTool;
        let result = tool
            .execute(json!({"query": "tool_stats"}), &ctx)
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("No tool execution data"));
    }

    #[tokio::test]
    async fn tool_stats_with_data() {
        let (_db, ctx, repo) = setup().await;
        for _ in 0..3 {
            repo.record("s1", "tool_success", "bash", 1.0, None)
                .await
                .unwrap();
        }
        repo.record("s1", "tool_failure", "bash", 0.0, None)
            .await
            .unwrap();

        let tool = FeedbackAnalyzeTool;
        let result = tool
            .execute(json!({"query": "tool_stats"}), &ctx)
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("bash"));
        assert!(result.output.contains("75.0%"));
    }

    #[tokio::test]
    async fn recent_empty() {
        let (_db, ctx, _repo) = setup().await;
        let tool = FeedbackAnalyzeTool;
        let result = tool
            .execute(json!({"query": "recent"}), &ctx)
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("No recent feedback"));
    }

    #[tokio::test]
    async fn recent_with_data() {
        let (_db, ctx, repo) = setup().await;
        repo.record("s1", "tool_success", "bash", 1.0, Some("ran ls"))
            .await
            .unwrap();

        let tool = FeedbackAnalyzeTool;
        let result = tool
            .execute(json!({"query": "recent", "limit": 10}), &ctx)
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("1 entries"));
        assert!(result.output.contains("tool_success"));
        assert!(result.output.contains("bash"));
    }

    #[tokio::test]
    async fn recent_respects_limit() {
        let (_db, ctx, repo) = setup().await;
        for i in 0..10 {
            repo.record("s1", "tool_success", &format!("t{i}"), 1.0, None)
                .await
                .unwrap();
        }

        let tool = FeedbackAnalyzeTool;
        let result = tool
            .execute(json!({"query": "recent", "limit": 3}), &ctx)
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("3 entries"));
    }

    #[tokio::test]
    async fn failures_empty() {
        let (_db, ctx, _repo) = setup().await;
        let tool = FeedbackAnalyzeTool;
        let result = tool
            .execute(json!({"query": "failures"}), &ctx)
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("No tool failures"));
    }

    #[tokio::test]
    async fn failures_with_data() {
        let (_db, ctx, repo) = setup().await;
        repo.record("s1", "tool_success", "bash", 1.0, None)
            .await
            .unwrap();
        repo.record("s1", "tool_failure", "edit", 0.0, Some("permission denied"))
            .await
            .unwrap();

        let tool = FeedbackAnalyzeTool;
        let result = tool
            .execute(json!({"query": "failures"}), &ctx)
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("1 entries"));
        assert!(result.output.contains("edit"));
        assert!(result.output.contains("permission denied"));
    }

    #[tokio::test]
    async fn unknown_query_type() {
        let (_db, ctx, _repo) = setup().await;
        let tool = FeedbackAnalyzeTool;
        let result = tool.execute(json!({"query": "bogus"}), &ctx).await.unwrap();
        assert!(!result.success);
        assert!(result_text(&result).contains("Unknown query type"));
    }

    #[tokio::test]
    async fn no_service_context() {
        let ctx = ToolExecutionContext::new(Uuid::new_v4());
        let tool = FeedbackAnalyzeTool;
        let result = tool
            .execute(json!({"query": "summary"}), &ctx)
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result_text(&result).contains("database"));
    }
}

// --- Self-Improve Tool Tests ---

mod self_improve_tool {
    use crate::brain::tools::self_improve::SelfImproveTool;
    use crate::brain::tools::{Tool, ToolExecutionContext};
    use crate::db::Database;
    use crate::services::ServiceContext;
    use serde_json::json;
    use uuid::Uuid;

    fn result_text(result: &crate::brain::tools::ToolResult) -> &str {
        if result.success {
            &result.output
        } else {
            result.error.as_deref().unwrap_or("")
        }
    }

    fn setup_ctx_no_db() -> ToolExecutionContext {
        let mut ctx = ToolExecutionContext::new(Uuid::new_v4());
        ctx.working_directory = std::env::temp_dir();
        ctx
    }

    async fn setup_ctx_with_db() -> (Database, ToolExecutionContext) {
        let db = Database::connect_in_memory().await.expect("in-memory DB");
        db.run_migrations().await.expect("migrations");
        let svc = ServiceContext::new(db.pool().clone());
        let mut ctx = ToolExecutionContext::new(Uuid::new_v4());
        ctx.working_directory = std::env::temp_dir();
        ctx.service_context = Some(svc);
        (db, ctx)
    }

    #[test]
    fn tool_metadata() {
        let tool = SelfImproveTool;
        assert_eq!(tool.name(), "self_improve");
        assert!(!tool.requires_approval()); // Autonomous — no human approval
        assert!(!tool.capabilities().is_empty());
    }

    #[test]
    fn no_approval_needed() {
        let tool = SelfImproveTool;
        assert!(!tool.requires_approval_for_input(&json!({"action": "apply"})));
        assert!(!tool.requires_approval_for_input(&json!({"action": "list"})));
    }

    #[tokio::test]
    async fn list_action() {
        let ctx = setup_ctx_no_db();
        let tool = SelfImproveTool;
        let result = tool.execute(json!({"action": "list"}), &ctx).await.unwrap();
        // list always succeeds — either reads file or reports it doesn't exist
        assert!(result.success);
    }

    #[tokio::test]
    async fn apply_missing_description() {
        let ctx = setup_ctx_no_db();
        let tool = SelfImproveTool;
        let result = tool
            .execute(
                json!({
                    "action": "apply",
                    "target_file": "SOUL.md",
                    "content": "test"
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result_text(&result).contains("description"));
    }

    #[tokio::test]
    async fn apply_writes_to_rsi_improvements() {
        let (_db, ctx) = setup_ctx_with_db().await;
        let tool = SelfImproveTool;
        let result = tool
            .execute(
                json!({
                    "action": "apply",
                    "target_file": "AGENTS.md",
                    "description": "Add retry logic to bash tool",
                    "rationale": "Frequent transient failures observed",
                    "content": "## Bash Retry\nAdd exponential backoff."
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("applied"));
        assert!(result.output.contains("Add retry logic"));

        // Verify rsi/improvements.md was written to ~/.opencrabs/
        let home = crate::config::opencrabs_home();
        let improvements =
            std::fs::read_to_string(home.join("rsi").join("improvements.md")).unwrap();
        assert!(improvements.contains("Add retry logic"));
        assert!(improvements.contains("Frequent transient failures"));
    }

    #[tokio::test]
    async fn apply_missing_fields() {
        let ctx = setup_ctx_no_db();
        let tool = SelfImproveTool;

        // Missing target_file
        let result = tool
            .execute(
                json!({
                    "action": "apply",
                    "description": "test",
                    "content": "test content"
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result_text(&result).contains("required"));

        // Missing content
        let result = tool
            .execute(
                json!({
                    "action": "apply",
                    "target_file": "SOUL.md",
                    "description": "test"
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!result.success);

        // Missing description
        let result = tool
            .execute(
                json!({
                    "action": "apply",
                    "target_file": "SOUL.md",
                    "content": "test"
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!result.success);
    }

    #[tokio::test]
    async fn apply_invalid_target_file() {
        let ctx = setup_ctx_no_db();
        let tool = SelfImproveTool;
        let result = tool
            .execute(
                json!({
                    "action": "apply",
                    "target_file": "EVIL.md",
                    "description": "test",
                    "content": "malicious content"
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result_text(&result).contains("must be one of"));
    }

    #[tokio::test]
    async fn apply_rejects_path_traversal() {
        let ctx = setup_ctx_no_db();
        let tool = SelfImproveTool;
        let result = tool
            .execute(
                json!({
                    "action": "apply",
                    "target_file": "../../../etc/passwd",
                    "description": "test",
                    "content": "test"
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result_text(&result).contains("must be one of"));
    }

    #[tokio::test]
    async fn apply_valid_brain_file() {
        let (_db, ctx) = setup_ctx_with_db().await;
        let tool = SelfImproveTool;

        let result = tool
            .execute(
                json!({
                    "action": "apply",
                    "target_file": "SOUL.md",
                    "description": "Add conciseness guideline",
                    "rationale": "Users consistently prefer shorter responses",
                    "content": "## Conciseness\nKeep responses under 3 sentences when possible."
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("applied"));
        assert!(result.output.contains("SOUL.md"));

        // Verify content was appended to SOUL.md in ~/.opencrabs/
        let home = crate::config::opencrabs_home();
        let soul = std::fs::read_to_string(home.join("SOUL.md")).unwrap();
        assert!(soul.contains("Conciseness"));

        // Verify rsi/improvements.md logged the change
        let improvements =
            std::fs::read_to_string(home.join("rsi").join("improvements.md")).unwrap();
        assert!(improvements.contains("SOUL.md"));
    }

    #[tokio::test]
    async fn apply_all_allowed_files_pass_whitelist() {
        let tool = SelfImproveTool;
        let allowed = [
            "SOUL.md",
            "USER.md",
            "AGENTS.md",
            "TOOLS.md",
            "CODE.md",
            "SECURITY.md",
            "MEMORY.md",
            "BOOT.md",
            "IDENTITY.md",
        ];

        let ctx = setup_ctx_no_db();
        for file in &allowed {
            let result = tool
                .execute(
                    json!({
                        "action": "apply",
                        "target_file": file,
                        "description": "test",
                        "content": "test"
                    }),
                    &ctx,
                )
                .await
                .unwrap();
            // Should NOT get "must be one of" error (may get other errors like file I/O)
            if !result.success {
                let err = result_text(&result);
                assert!(
                    !err.contains("must be one of"),
                    "{file} should be allowed but got: {err}",
                );
            }
        }
    }

    #[tokio::test]
    async fn unknown_action() {
        let ctx = setup_ctx_no_db();
        let tool = SelfImproveTool;
        let result = tool
            .execute(json!({"action": "delete"}), &ctx)
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result_text(&result).contains("Unknown action"));
    }

    #[tokio::test]
    async fn apply_without_rationale() {
        let (_db, ctx) = setup_ctx_with_db().await;
        let tool = SelfImproveTool;
        let result = tool
            .execute(
                json!({
                    "action": "apply",
                    "target_file": "AGENTS.md",
                    "description": "Improve error messages",
                    "content": "## Better Errors\nReturn actionable hints."
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.success);
        // Rationale defaults to "(none)"
        let home = crate::config::opencrabs_home();
        let improvements =
            std::fs::read_to_string(home.join("rsi").join("improvements.md")).unwrap();
        assert!(improvements.contains("(none)"));
    }
}

// --- Integration: Record → Analyze round-trip ---

mod rsi_integration {
    use crate::brain::tools::feedback_analyze::FeedbackAnalyzeTool;
    use crate::brain::tools::feedback_record::FeedbackRecordTool;
    use crate::brain::tools::{Tool, ToolExecutionContext};
    use crate::db::Database;
    use crate::services::ServiceContext;
    use serde_json::json;
    use uuid::Uuid;

    async fn setup() -> (Database, ToolExecutionContext) {
        let db = Database::connect_in_memory().await.expect("in-memory DB");
        db.run_migrations().await.expect("migrations");
        let svc = ServiceContext::new(db.pool().clone());
        let mut ctx = ToolExecutionContext::new(Uuid::new_v4());
        ctx.service_context = Some(svc);
        (db, ctx)
    }

    #[tokio::test]
    async fn record_then_analyze_summary() {
        let (_db, ctx) = setup().await;
        let record = FeedbackRecordTool;
        let analyze = FeedbackAnalyzeTool;

        // Record several events
        record
            .execute(
                json!({"event_type": "tool_success", "dimension": "bash", "value": 1.0}),
                &ctx,
            )
            .await
            .unwrap();
        record
            .execute(
                json!({"event_type": "tool_success", "dimension": "read", "value": 1.0}),
                &ctx,
            )
            .await
            .unwrap();
        record
            .execute(
                json!({"event_type": "tool_failure", "dimension": "edit", "value": 0.0, "metadata": "file locked"}),
                &ctx,
            )
            .await
            .unwrap();

        // Analyze summary
        let result = analyze
            .execute(json!({"query": "summary"}), &ctx)
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("3 total events"));
        assert!(result.output.contains("tool_success"));
        assert!(result.output.contains("tool_failure"));
    }

    #[tokio::test]
    async fn record_then_analyze_tool_stats() {
        let (_db, ctx) = setup().await;
        let record = FeedbackRecordTool;
        let analyze = FeedbackAnalyzeTool;

        // 5 bash successes, 1 bash failure
        for _ in 0..5 {
            record
                .execute(
                    json!({"event_type": "tool_success", "dimension": "bash"}),
                    &ctx,
                )
                .await
                .unwrap();
        }
        record
            .execute(
                json!({"event_type": "tool_failure", "dimension": "bash"}),
                &ctx,
            )
            .await
            .unwrap();

        let result = analyze
            .execute(json!({"query": "tool_stats"}), &ctx)
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("bash"));
        assert!(result.output.contains("83.3%"));
    }

    #[tokio::test]
    async fn record_then_analyze_failures() {
        let (_db, ctx) = setup().await;
        let record = FeedbackRecordTool;
        let analyze = FeedbackAnalyzeTool;

        record
            .execute(
                json!({"event_type": "tool_failure", "dimension": "edit", "metadata": "permission denied"}),
                &ctx,
            )
            .await
            .unwrap();
        record
            .execute(
                json!({"event_type": "tool_success", "dimension": "bash"}),
                &ctx,
            )
            .await
            .unwrap();

        let result = analyze
            .execute(json!({"query": "failures"}), &ctx)
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("1 entries"));
        assert!(result.output.contains("edit"));
        assert!(result.output.contains("permission denied"));
    }

    #[tokio::test]
    async fn edge_case_high_volume() {
        let (_db, ctx) = setup().await;
        let record = FeedbackRecordTool;
        let analyze = FeedbackAnalyzeTool;

        // Record 100 events
        for i in 0..100 {
            let event_type = if i % 5 == 0 {
                "tool_failure"
            } else {
                "tool_success"
            };
            record
                .execute(
                    json!({
                        "event_type": event_type,
                        "dimension": format!("tool_{}", i % 10),
                        "value": if event_type == "tool_success" { 1.0 } else { 0.0 }
                    }),
                    &ctx,
                )
                .await
                .unwrap();
        }

        // Summary should show 100 total
        let result = analyze
            .execute(json!({"query": "summary"}), &ctx)
            .await
            .unwrap();
        assert!(result.output.contains("100 total events"));

        // Tool stats should show dimensions
        let result = analyze
            .execute(json!({"query": "tool_stats"}), &ctx)
            .await
            .unwrap();
        assert!(result.success);

        // Recent with limit
        let result = analyze
            .execute(json!({"query": "recent", "limit": 5}), &ctx)
            .await
            .unwrap();
        assert!(result.output.contains("5 entries"));
    }
}

// --- User Correction Detection Tests ---

mod user_correction_detection {
    // Test the is_user_correction function from tool_loop
    use crate::brain::agent::service::tool_loop::is_user_correction;

    #[test]
    fn detects_simple_no() {
        assert!(is_user_correction("no, that's wrong"));
        assert!(is_user_correction("No. Try something else."));
        assert!(is_user_correction("no! stop doing that"));
    }

    #[test]
    fn detects_wrong() {
        assert!(is_user_correction("that's wrong"));
        assert!(is_user_correction("Wrong answer"));
    }

    #[test]
    fn detects_not_what_i_meant() {
        assert!(is_user_correction("that's not what I wanted"));
        assert!(is_user_correction("thats not what i asked for"));
    }

    #[test]
    fn detects_try_again() {
        assert!(is_user_correction("try again please"));
        assert!(is_user_correction("redo this"));
    }

    #[test]
    fn detects_broke_it() {
        assert!(is_user_correction("you broke everything"));
        assert!(is_user_correction("broke it again"));
    }

    #[test]
    fn detects_not_working() {
        assert!(is_user_correction("doesn't work"));
        assert!(is_user_correction("it's not working"));
        assert!(is_user_correction("didn't work"));
    }

    #[test]
    fn detects_fix_commands() {
        assert!(is_user_correction("fix it"));
        assert!(is_user_correction("fix this please"));
    }

    #[test]
    fn detects_stop_dont() {
        assert!(is_user_correction("stop doing that"));
        assert!(is_user_correction("don't do that again"));
    }

    #[test]
    fn detects_i_said() {
        assert!(is_user_correction("i said to use the other approach"));
        assert!(is_user_correction("i asked for something different"));
    }

    #[test]
    fn ignores_normal_messages() {
        assert!(!is_user_correction("please add a login form"));
        assert!(!is_user_correction("how does the database work?"));
        assert!(!is_user_correction("can you explain this function?"));
        assert!(!is_user_correction("create a new file called test.rs"));
    }

    #[test]
    fn ignores_long_messages() {
        // Messages >500 chars are assumed to be new instructions
        let long_msg = "x".repeat(501);
        assert!(!is_user_correction(&long_msg));
    }

    #[test]
    fn ignores_very_short() {
        assert!(!is_user_correction(""));
        assert!(!is_user_correction("x"));
    }

    #[test]
    fn case_insensitive() {
        assert!(is_user_correction("WRONG"));
        assert!(is_user_correction("No, That's Not Right"));
        assert!(is_user_correction("FIX IT"));
    }

    #[test]
    fn nope_detection() {
        assert!(is_user_correction("nope, try something else"));
    }

    #[test]
    fn revert_undo() {
        assert!(is_user_correction("revert those changes"));
        assert!(is_user_correction("undo what you just did"));
    }
}

// --- Feedback Digest Tests ---

mod feedback_digest {
    use crate::brain::prompt_builder::build_feedback_digest;
    use crate::db::Database;
    use crate::db::repository::FeedbackLedgerRepository;

    #[tokio::test]
    async fn returns_none_for_empty_ledger() {
        let db = Database::connect_in_memory().await.expect("in-memory DB");
        db.run_migrations().await.expect("migrations");
        let result = build_feedback_digest(db.pool().clone()).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn returns_none_under_10_events() {
        let db = Database::connect_in_memory().await.expect("in-memory DB");
        db.run_migrations().await.expect("migrations");
        let repo = FeedbackLedgerRepository::new(db.pool().clone());
        for i in 0..9 {
            repo.record("s1", "tool_success", &format!("t{i}"), 1.0, None)
                .await
                .unwrap();
        }
        let result = build_feedback_digest(db.pool().clone()).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn returns_digest_with_enough_data() {
        let db = Database::connect_in_memory().await.expect("in-memory DB");
        db.run_migrations().await.expect("migrations");
        let repo = FeedbackLedgerRepository::new(db.pool().clone());
        // 8 successes + 4 failures = 12 total (>10 threshold)
        for _ in 0..8 {
            repo.record("s1", "tool_success", "bash", 1.0, None)
                .await
                .unwrap();
        }
        for _ in 0..4 {
            repo.record("s1", "tool_failure", "edit", 0.0, Some("file locked"))
                .await
                .unwrap();
        }

        let result = build_feedback_digest(db.pool().clone()).await;
        assert!(result.is_some());
        let digest = result.unwrap();
        assert!(digest.contains("Performance History"));
        assert!(digest.contains("12")); // total count
        assert!(digest.contains("edit")); // failing tool
        assert!(digest.contains("file locked")); // failure metadata
    }

    #[tokio::test]
    async fn includes_user_corrections() {
        let db = Database::connect_in_memory().await.expect("in-memory DB");
        db.run_migrations().await.expect("migrations");
        let repo = FeedbackLedgerRepository::new(db.pool().clone());
        for _ in 0..10 {
            repo.record("s1", "tool_success", "bash", 1.0, None)
                .await
                .unwrap();
        }
        repo.record(
            "s1",
            "user_correction",
            "user_message",
            1.0,
            Some("no, wrong"),
        )
        .await
        .unwrap();

        let result = build_feedback_digest(db.pool().clone()).await;
        assert!(result.is_some());
        let digest = result.unwrap();
        assert!(digest.contains("User corrections recorded: 1"));
    }

    #[tokio::test]
    async fn skips_high_success_rate_tools() {
        let db = Database::connect_in_memory().await.expect("in-memory DB");
        db.run_migrations().await.expect("migrations");
        let repo = FeedbackLedgerRepository::new(db.pool().clone());
        // 95% success rate — should NOT appear in "notable failures"
        for _ in 0..19 {
            repo.record("s1", "tool_success", "bash", 1.0, None)
                .await
                .unwrap();
        }
        repo.record("s1", "tool_failure", "bash", 0.0, None)
            .await
            .unwrap();

        let result = build_feedback_digest(db.pool().clone()).await;
        assert!(result.is_some());
        let digest = result.unwrap();
        // bash has 95% success rate, which is >= 90% threshold, so it should NOT
        // appear under "notable failure rates"
        assert!(!digest.contains("notable failure rates"));
    }
}

// --- Opportunity hash dedup (cycle telemetry stability) ---
// 2026-05-18: cycle #426 re-printed cycle #425's top-5 corrections /
// errors verbatim in the TUI because the engine emitted
// `ImprovementOpportunity` notifications inline, regardless of whether
// the assembled list had changed. `hash_opportunities` is the dedup
// anchor — same descriptions → same hex → cycle short-circuits.
#[cfg(test)]
mod hash_opportunities {
    use crate::brain::rsi::hash_opportunities;

    #[test]
    fn identical_lists_hash_identically() {
        let a = vec![
            "50 user corrections recorded.\n  - session=abc, time=...".to_string(),
            "20 provider errors recorded.\n  - session=def, time=...".to_string(),
        ];
        let b = a.clone();
        assert_eq!(hash_opportunities(&a), hash_opportunities(&b));
    }

    #[test]
    fn different_lists_hash_differently() {
        let a = vec!["50 user corrections recorded.".to_string()];
        let b = vec!["51 user corrections recorded.".to_string()];
        assert_ne!(hash_opportunities(&a), hash_opportunities(&b));
    }

    #[test]
    fn reordered_top_5_changes_hash() {
        // Even a single recent event that shifts the top-5 slice must
        // re-enable the full emission path. The mitigation the user
        // asked for: don't collapse "same count, different events".
        let a = vec!["recent:\n  - session=aaa\n  - session=bbb".to_string()];
        let b = vec!["recent:\n  - session=bbb\n  - session=aaa".to_string()];
        assert_ne!(hash_opportunities(&a), hash_opportunities(&b));
    }

    #[test]
    fn merge_vs_two_entries_hash_differently() {
        // Sentinel-joined hashing must not let two adjacent
        // descriptions collide with one merged-content description.
        let two = vec!["alpha".to_string(), "beta".to_string()];
        let one_merged = vec!["alphabeta".to_string()];
        assert_ne!(hash_opportunities(&two), hash_opportunities(&one_merged));
    }

    #[test]
    fn empty_list_has_stable_hash() {
        // Empty input must produce a deterministic baseline hash so
        // back-to-back zero-opportunity cycles dedup cleanly.
        let a: Vec<String> = Vec::new();
        let b: Vec<String> = Vec::new();
        assert_eq!(hash_opportunities(&a), hash_opportunities(&b));
        assert!(!hash_opportunities(&a).is_empty());
    }

    #[test]
    fn whitespace_change_breaks_dedup() {
        // A single character delta is enough to trip the hash and
        // re-enable emission — extra-conservative on the
        // "don't miss anything" side.
        let a = vec!["50 user corrections recorded.".to_string()];
        let b = vec!["50  user corrections recorded.".to_string()];
        assert_ne!(hash_opportunities(&a), hash_opportunities(&b));
    }
}
