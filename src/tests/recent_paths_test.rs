//! Tests for the persistent `recent_paths` store and the prompt-augmentation
//! that surfaces it.
//!
//! The store is keyed on `(working_directory, path)` (both in `~/...`
//! collapsed form) so a new session on the same project can re-anchor
//! on real paths the agent already verified, instead of hallucinating
//! directory layouts (the 2026-04-26 myapp `lib/screens` failure
//! pattern that drove this feature).

mod repository {
    use crate::db::Database;
    use crate::db::repository::RecentPathsRepository;

    async fn setup() -> (Database, RecentPathsRepository) {
        let db = Database::connect_in_memory()
            .await
            .expect("Failed to create database");
        db.run_migrations().await.expect("Failed to run migrations");
        let repo = RecentPathsRepository::new(db.pool().clone());
        (db, repo)
    }

    #[tokio::test]
    async fn empty_dir_returns_empty_vec() {
        let (_db, repo) = setup().await;
        let got = repo
            .top_for_dir("~/srv/dart/myapp", 12)
            .await
            .expect("query");
        assert!(got.is_empty());
    }

    #[tokio::test]
    async fn record_then_read_round_trip() {
        let (_db, repo) = setup().await;
        let wd = "~/srv/dart/myapp";
        let path =
            "~/srv/dart/myapp/lib/presentation/propositions_screen/propositions_screen.dart";
        repo.record(wd, path).await.expect("record");
        let got = repo.top_for_dir(wd, 12).await.expect("query");
        assert_eq!(got, vec![path.to_string()]);
    }

    #[tokio::test]
    async fn duplicate_record_keeps_single_entry() {
        // The whole point of the (working_directory, path) primary key:
        // re-recording the same path must not produce duplicate rows.
        let (_db, repo) = setup().await;
        let wd = "~/srv/rs/opencrabs";
        let path = "~/srv/rs/opencrabs/src/main.rs";
        repo.record(wd, path).await.expect("first record");
        repo.record(wd, path).await.expect("second record");
        repo.record(wd, path).await.expect("third record");
        let got = repo.top_for_dir(wd, 12).await.expect("query");
        assert_eq!(got, vec![path.to_string()]);
    }

    #[tokio::test]
    async fn paths_isolated_per_working_directory() {
        // Two projects must not see each other's paths — the index is
        // keyed on working_directory.
        let (_db, repo) = setup().await;
        repo.record("~/proj/a", "~/proj/a/lib/main.dart")
            .await
            .expect("a");
        repo.record("~/proj/b", "~/proj/b/src/main.rs")
            .await
            .expect("b");
        let a = repo.top_for_dir("~/proj/a", 12).await.expect("a query");
        let b = repo.top_for_dir("~/proj/b", 12).await.expect("b query");
        assert_eq!(a, vec!["~/proj/a/lib/main.dart".to_string()]);
        assert_eq!(b, vec!["~/proj/b/src/main.rs".to_string()]);
    }

    #[tokio::test]
    async fn most_recent_first_ordering() {
        // top_for_dir is ORDER BY last_accessed DESC. Sleep 1s between
        // writes so the integer-second timestamps actually differ —
        // strftime('%s', 'now') has 1-second resolution.
        let (_db, repo) = setup().await;
        let wd = "~/proj";
        repo.record(wd, "~/proj/oldest.txt").await.expect("oldest");
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        repo.record(wd, "~/proj/middle.txt").await.expect("middle");
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        repo.record(wd, "~/proj/newest.txt").await.expect("newest");
        let got = repo.top_for_dir(wd, 12).await.expect("query");
        assert_eq!(
            got,
            vec![
                "~/proj/newest.txt".to_string(),
                "~/proj/middle.txt".to_string(),
                "~/proj/oldest.txt".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn re_recording_path_moves_it_to_front() {
        // ON CONFLICT DO UPDATE bumps last_accessed, so the touched
        // path should jump back to the head of the list even if it
        // was the oldest before.
        let (_db, repo) = setup().await;
        let wd = "~/proj";
        repo.record(wd, "~/proj/a.txt").await.expect("a");
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        repo.record(wd, "~/proj/b.txt").await.expect("b");
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        // Touch a.txt again — it should now be most recent.
        repo.record(wd, "~/proj/a.txt").await.expect("a again");
        let got = repo.top_for_dir(wd, 12).await.expect("query");
        assert_eq!(
            got,
            vec!["~/proj/a.txt".to_string(), "~/proj/b.txt".to_string()]
        );
    }

    #[tokio::test]
    async fn limit_caps_returned_rows() {
        let (_db, repo) = setup().await;
        let wd = "~/proj";
        for i in 0..20 {
            repo.record(wd, &format!("~/proj/file_{:02}.txt", i))
                .await
                .expect("record");
        }
        let got = repo.top_for_dir(wd, 5).await.expect("query");
        assert_eq!(got.len(), 5);
    }
}

// --- Prompt-augmentation tests ---
//
// `augment_system_with_recent_paths` is the pure render-time function:
// it appends the "Recently accessed in this project" section to the
// system prompt, but only for paths that aren't already mentioned in
// the live messages (so we don't double-list paths the agent just
// touched in the same uncompacted session).

mod augment {
    use crate::brain::agent::service::AgentService;
    use crate::brain::provider::{ContentBlock, Message, Role};

    fn tool_use_msg(input_path: &str) -> Message {
        Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "read_file".into(),
                input: serde_json::json!({ "path": input_path }),
            }],
        }
    }

    fn tool_result_msg(body: &str) -> Message {
        Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "tu_1".into(),
                content: body.into(),
                is_error: None,
            }],
        }
    }

    #[test]
    fn empty_recent_paths_returns_base_unchanged() {
        let base = Some("BASE PROMPT".to_string());
        let got = AgentService::augment_system_with_recent_paths(base.clone(), &[], &[]);
        assert_eq!(got, base);
    }

    #[test]
    fn empty_recent_paths_with_no_base_returns_none() {
        let got = AgentService::augment_system_with_recent_paths(None, &[], &[]);
        assert_eq!(got, None);
    }

    #[test]
    fn surfaces_path_when_not_in_messages() {
        // Cold start: prior session figured out the myapp layout,
        // but this fresh session has no messages mentioning it yet.
        // The buffer should be surfaced.
        let recent = vec![
            "~/srv/dart/myapp/lib/presentation/propositions_screen/propositions_screen.dart"
                .to_string(),
        ];
        let got = AgentService::augment_system_with_recent_paths(Some("BASE".into()), &recent, &[])
            .expect("section emitted");
        assert!(got.starts_with("BASE"));
        assert!(got.contains("Recently accessed in this project"));
        assert!(got.contains(
            "~/srv/dart/myapp/lib/presentation/propositions_screen/propositions_screen.dart"
        ));
    }

    #[test]
    fn skips_path_already_in_message_text() {
        // Same uncompacted session: the path was just mentioned by
        // the assistant in plain text. Don't double-list it.
        let recent = vec!["~/srv/rs/opencrabs/src/main.rs".to_string()];
        let messages = vec![Message::assistant(
            "I'll edit ~/srv/rs/opencrabs/src/main.rs next.",
        )];
        let got =
            AgentService::augment_system_with_recent_paths(Some("BASE".into()), &recent, &messages);
        // Filter ate every recent path → no augmentation, base passes through.
        assert_eq!(got, Some("BASE".into()));
    }

    #[test]
    fn skips_path_already_in_tool_use_input() {
        // The path lives inside ToolUse.input (e.g. {"path":"..."}),
        // not the message text — must still be filtered out.
        let recent = vec!["~/proj/lib/main.dart".to_string()];
        let messages = vec![tool_use_msg("~/proj/lib/main.dart")];
        let got =
            AgentService::augment_system_with_recent_paths(Some("BASE".into()), &recent, &messages);
        assert_eq!(got, Some("BASE".into()));
    }

    #[test]
    fn skips_path_already_in_tool_result_content() {
        // grep / ls dump matched paths into ToolResult.content. If
        // the agent already saw it there, no need to re-surface.
        let recent = vec!["~/proj/src/api.rs".to_string()];
        let messages = vec![tool_result_msg(
            "~/proj/src/api.rs:42: pub fn handle_request() {}",
        )];
        let got =
            AgentService::augment_system_with_recent_paths(Some("BASE".into()), &recent, &messages);
        assert_eq!(got, Some("BASE".into()));
    }

    #[test]
    fn matches_case_insensitively() {
        // The model sometimes capitalizes paths in narration ("See
        // SRC/Main.rs"). The filter lowercases both sides so we don't
        // emit a duplicate just because of casing.
        let recent = vec!["~/proj/src/main.rs".to_string()];
        let messages = vec![Message::assistant("Reading ~/Proj/SRC/Main.rs now")];
        let got =
            AgentService::augment_system_with_recent_paths(Some("BASE".into()), &recent, &messages);
        assert_eq!(got, Some("BASE".into()));
    }

    #[test]
    fn keeps_only_paths_missing_from_context() {
        // Mixed case: one recent path is already in context, one isn't.
        // Only the missing one gets surfaced.
        let recent = vec![
            "~/proj/lib/known.dart".to_string(),
            "~/proj/lib/forgotten.dart".to_string(),
        ];
        let messages = vec![Message::user("Please look at ~/proj/lib/known.dart")];
        let got =
            AgentService::augment_system_with_recent_paths(Some("BASE".into()), &recent, &messages)
                .expect("section emitted");
        assert!(!got.contains("known.dart"));
        assert!(got.contains("~/proj/lib/forgotten.dart"));
    }

    #[test]
    fn appends_with_separating_newline_when_base_is_dense() {
        // Base prompt without trailing newline must still be cleanly
        // separated from the appended section.
        let recent = vec!["~/proj/x.txt".to_string()];
        let got = AgentService::augment_system_with_recent_paths(
            Some("BASE NO TRAILING NEWLINE".into()),
            &recent,
            &[],
        )
        .expect("section emitted");
        assert!(got.contains("BASE NO TRAILING NEWLINE\n"));
        assert!(got.contains("Recently accessed in this project"));
    }

    #[test]
    fn no_base_emits_section_alone_when_paths_survive() {
        let recent = vec!["~/proj/x.txt".to_string()];
        let got = AgentService::augment_system_with_recent_paths(None, &recent, &[])
            .expect("section emitted");
        assert!(got.contains("Recently accessed in this project"));
        assert!(got.contains("~/proj/x.txt"));
    }
}
