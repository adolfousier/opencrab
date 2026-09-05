//! Channel Search & Message Capture Tests
//!
//! Tests for ChannelMessageRepository CRUD, multi-chat/multi-channel queries,
//! and the ChannelSearchTool agent operations.

// --- Repository Tests ---

mod repository {
    use crate::db::Database;
    use crate::db::models::ChannelMessage;
    use crate::db::repository::channel_message::ChannelMessageRepository;

    async fn setup() -> (Database, ChannelMessageRepository) {
        let db = Database::connect_in_memory()
            .await
            .expect("Failed to create database");
        db.run_migrations().await.expect("Failed to run migrations");
        let repo = ChannelMessageRepository::new(db.pool().clone());
        (db, repo)
    }

    fn msg(
        channel: &str,
        chat_id: &str,
        chat_name: &str,
        sender: &str,
        content: &str,
    ) -> ChannelMessage {
        ChannelMessage::new(
            channel.into(),
            chat_id.into(),
            Some(chat_name.into()),
            "user1".into(),
            sender.into(),
            content.into(),
            "text".into(),
            None,
        )
    }

    #[tokio::test]
    async fn content_by_platform_message_id_returns_exact_match() {
        let (_db, repo) = setup().await;

        // Two bot replies in the same DM, each with its own Telegram message id.
        // The newer one would win any "most recent bot message" heuristic, so
        // this proves the lookup keys on the id, not recency.
        let older = ChannelMessage::new(
            "telegram".into(),
            "7711740248".into(),
            Some("DM".into()),
            "bot:opencrabs".into(),
            "OpenCrabs".into(),
            "Done. Updated the GitHub stats cron.".into(),
            "text".into(),
            Some("1001".into()),
        );
        let newer = ChannelMessage::new(
            "telegram".into(),
            "7711740248".into(),
            Some("DM".into()),
            "bot:opencrabs".into(),
            "OpenCrabs".into(),
            "Yeah I can see it.".into(),
            "text".into(),
            Some("1002".into()),
        );
        repo.insert(&older).await.unwrap();
        repo.insert(&newer).await.unwrap();

        // Replying to the OLDER message must recover the OLDER content, even
        // though a newer bot message exists.
        let got = repo
            .content_by_platform_message_id("telegram", "7711740248", "1001")
            .await
            .unwrap();
        assert_eq!(got.as_deref(), Some("Done. Updated the GitHub stats cron."));

        // Unknown id → None (caller falls back to its heuristic).
        let miss = repo
            .content_by_platform_message_id("telegram", "7711740248", "9999")
            .await
            .unwrap();
        assert_eq!(miss, None);
    }

    #[tokio::test]
    async fn test_insert_and_recent() {
        let (_db, repo) = setup().await;
        let m = msg("telegram", "-100111", "Group A", "Alice", "Hello world");
        repo.insert(&m).await.unwrap();

        let recent = repo
            .recent(Some("telegram"), "-100111", 10, None, None)
            .await
            .unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].content, "Hello world");
        assert_eq!(recent[0].sender_name, "Alice");
        assert_eq!(recent[0].channel, "telegram");
    }

    #[tokio::test]
    async fn test_recent_respects_limit() {
        let (_db, repo) = setup().await;
        for i in 0..10 {
            let m = msg(
                "telegram",
                "-100111",
                "Group A",
                "Alice",
                &format!("msg {i}"),
            );
            repo.insert(&m).await.unwrap();
        }

        let recent = repo
            .recent(Some("telegram"), "-100111", 3, None, None)
            .await
            .unwrap();
        assert_eq!(recent.len(), 3);
    }

    #[tokio::test]
    async fn test_recent_without_channel_filter() {
        let (_db, repo) = setup().await;
        repo.insert(&msg(
            "telegram",
            "-100111",
            "TG Group",
            "Alice",
            "from telegram",
        ))
        .await
        .unwrap();
        repo.insert(&msg(
            "discord",
            "-100111",
            "DC Group",
            "Bob",
            "from discord",
        ))
        .await
        .unwrap();

        // Same chat_id, no channel filter — both returned
        let recent = repo.recent(None, "-100111", 10, None, None).await.unwrap();
        assert_eq!(recent.len(), 2);
    }

    #[tokio::test]
    async fn test_recent_filters_by_channel() {
        let (_db, repo) = setup().await;
        repo.insert(&msg("telegram", "-100111", "TG Group", "Alice", "tg msg"))
            .await
            .unwrap();
        repo.insert(&msg("discord", "-100222", "DC Chan", "Bob", "dc msg"))
            .await
            .unwrap();

        let tg = repo
            .recent(Some("telegram"), "-100111", 10, None, None)
            .await
            .unwrap();
        assert_eq!(tg.len(), 1);
        assert_eq!(tg[0].content, "tg msg");

        let dc = repo
            .recent(Some("discord"), "-100111", 10, None, None)
            .await
            .unwrap();
        assert_eq!(dc.len(), 0);
    }

    #[tokio::test]
    async fn test_search_by_content() {
        let (_db, repo) = setup().await;
        repo.insert(&msg(
            "telegram",
            "-100111",
            "Group",
            "Alice",
            "the quick brown fox",
        ))
        .await
        .unwrap();
        repo.insert(&msg(
            "telegram",
            "-100111",
            "Group",
            "Bob",
            "lazy dog jumps",
        ))
        .await
        .unwrap();
        repo.insert(&msg("telegram", "-100111", "Group", "Carol", "hello world"))
            .await
            .unwrap();

        let results = repo
            .search(Some("telegram"), Some("-100111"), "fox", 10, None)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].sender_name, "Alice");
    }

    #[tokio::test]
    async fn test_search_across_chats() {
        let (_db, repo) = setup().await;
        repo.insert(&msg(
            "telegram",
            "-100111",
            "Group A",
            "Alice",
            "deploy failed",
        ))
        .await
        .unwrap();
        repo.insert(&msg(
            "telegram",
            "-100222",
            "Group B",
            "Bob",
            "deploy succeeded",
        ))
        .await
        .unwrap();
        repo.insert(&msg(
            "slack",
            "C999",
            "General",
            "Carol",
            "deploy in progress",
        ))
        .await
        .unwrap();

        // Search all channels, all chats
        let results = repo.search(None, None, "deploy", 10, None).await.unwrap();
        assert_eq!(results.len(), 3);

        // Search telegram only, all chats
        let results = repo
            .search(Some("telegram"), None, "deploy", 10, None)
            .await
            .unwrap();
        assert_eq!(results.len(), 2);

        // Search specific chat only
        let results = repo
            .search(None, Some("-100111"), "deploy", 10, None)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn test_search_no_match() {
        let (_db, repo) = setup().await;
        repo.insert(&msg("telegram", "-100111", "Group", "Alice", "hello"))
            .await
            .unwrap();

        let results = repo
            .search(Some("telegram"), Some("-100111"), "nonexistent", 10, None)
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_list_chats() {
        let (_db, repo) = setup().await;
        repo.insert(&msg("telegram", "-100111", "Group A", "Alice", "msg 1"))
            .await
            .unwrap();
        repo.insert(&msg("telegram", "-100111", "Group A", "Bob", "msg 2"))
            .await
            .unwrap();
        repo.insert(&msg("telegram", "-100222", "Group B", "Carol", "msg 3"))
            .await
            .unwrap();
        repo.insert(&msg("discord", "DC001", "Server Chan", "Dave", "msg 4"))
            .await
            .unwrap();

        // All channels
        let chats = repo.list_chats(None).await.unwrap();
        assert_eq!(chats.len(), 3);

        // Telegram only
        let chats = repo.list_chats(Some("telegram")).await.unwrap();
        assert_eq!(chats.len(), 2);

        // Find Group A — should have 2 messages
        let group_a = chats
            .iter()
            .find(|c| c.channel_chat_id == "-100111")
            .unwrap();
        assert_eq!(group_a.message_count, 2);
        assert_eq!(group_a.channel_chat_name.as_deref(), Some("Group A"));

        // Discord only
        let chats = repo.list_chats(Some("discord")).await.unwrap();
        assert_eq!(chats.len(), 1);
        assert_eq!(chats[0].message_count, 1);
    }

    #[tokio::test]
    async fn test_list_chats_empty() {
        let (_db, repo) = setup().await;
        let chats = repo.list_chats(None).await.unwrap();
        assert!(chats.is_empty());
    }

    #[tokio::test]
    async fn test_duplicate_insert_ignored() {
        let (_db, repo) = setup().await;
        let m = msg("telegram", "-100111", "Group", "Alice", "hello");
        repo.insert(&m).await.unwrap();
        // Same ID again — INSERT OR IGNORE
        repo.insert(&m).await.unwrap();

        let recent = repo
            .recent(Some("telegram"), "-100111", 10, None, None)
            .await
            .unwrap();
        assert_eq!(recent.len(), 1);
    }

    #[tokio::test]
    async fn test_message_fields_roundtrip() {
        let (_db, repo) = setup().await;
        let m = ChannelMessage::new(
            "slack".into(),
            "C123".into(),
            Some("general".into()),
            "U456".into(),
            "Bob".into(),
            "test content".into(),
            "text".into(),
            Some("ts_789".into()),
        );
        let id = m.id;
        repo.insert(&m).await.unwrap();

        let recent = repo
            .recent(Some("slack"), "C123", 1, None, None)
            .await
            .unwrap();
        assert_eq!(recent.len(), 1);
        let r = &recent[0];
        assert_eq!(r.id, id);
        assert_eq!(r.channel, "slack");
        assert_eq!(r.channel_chat_id, "C123");
        assert_eq!(r.channel_chat_name.as_deref(), Some("general"));
        assert_eq!(r.sender_id, "U456");
        assert_eq!(r.sender_name, "Bob");
        assert_eq!(r.content, "test content");
        assert_eq!(r.message_type, "text");
        assert_eq!(r.platform_message_id.as_deref(), Some("ts_789"));
    }

    // Reconciling a streamed/edited peer-bot message: the first frame is
    // stored, then a later edit rewrites the row to the final text so group
    // history reflects the completed message, not the partial.
    #[tokio::test]
    async fn test_update_content_reconciles_edited_message() {
        let (_db, repo) = setup().await;
        let frame = ChannelMessage::new(
            "telegram".into(),
            "-100111".into(),
            Some("Group".into()),
            "u9".into(),
            "Peer".into(),
            "Confidence assessment: 1. HIGH — will produce a real signal. The".into(),
            "text".into(),
            Some("msg_42".into()),
        );
        repo.insert(&frame).await.unwrap();

        let final_text = "Confidence assessment: 1. HIGH — will produce a real signal. \
                          The plan is a concrete artifact.";
        let updated = repo
            .update_content("telegram", "-100111", "msg_42", final_text)
            .await
            .unwrap();
        assert_eq!(updated, 1, "the stored frame should be rewritten");

        let recent = repo
            .recent(Some("telegram"), "-100111", 10, None, None)
            .await
            .unwrap();
        assert_eq!(recent.len(), 1, "reconcile updates in place, no new row");
        assert_eq!(recent[0].content, final_text);
    }

    #[tokio::test]
    async fn test_update_content_no_match_is_zero_rows() {
        let (_db, repo) = setup().await;
        let updated = repo
            .update_content("telegram", "-100111", "never_stored", "x")
            .await
            .unwrap();
        assert_eq!(updated, 0);
    }

    #[tokio::test]
    async fn test_recent_filters_by_thread_id() {
        let (_db, repo) = setup().await;

        // Insert messages in two different forum topics of the same chat
        let m1 = ChannelMessage::new(
            "telegram".into(),
            "-100111".into(),
            Some("Kanban Board".into()),
            "u1".into(),
            "Alice".into(),
            "topic A message".into(),
            "text".into(),
            None,
        )
        .with_thread(Some("2411".to_string()), Some("Topic A".into()));
        let m2 = ChannelMessage::new(
            "telegram".into(),
            "-100111".into(),
            Some("Kanban Board".into()),
            "u2".into(),
            "Bob".into(),
            "topic B message".into(),
            "text".into(),
            None,
        )
        .with_thread(Some("2614".to_string()), Some("Topic B".into()));
        let m3 = ChannelMessage::new(
            "telegram".into(),
            "-100111".into(),
            Some("Kanban Board".into()),
            "u1".into(),
            "Alice".into(),
            "another topic A message".into(),
            "text".into(),
            None,
        )
        .with_thread(Some("2411".to_string()), Some("Topic A".into()));

        repo.insert(&m1).await.unwrap();
        repo.insert(&m2).await.unwrap();
        repo.insert(&m3).await.unwrap();

        // Without thread_id filter: all 3 messages returned
        let all = repo
            .recent(Some("telegram"), "-100111", 10, None, None)
            .await
            .unwrap();
        assert_eq!(
            all.len(),
            3,
            "no thread_id filter should return all messages"
        );

        // With thread_id filter for topic A: only 2 messages
        let topic_a = repo
            .recent(Some("telegram"), "-100111", 10, Some("2411"), None)
            .await
            .unwrap();
        assert_eq!(
            topic_a.len(),
            2,
            "thread_id filter should return only topic A messages"
        );
        for msg in &topic_a {
            assert_eq!(msg.thread_id.as_deref(), Some("2411"));
        }

        // With thread_id filter for topic B: only 1 message
        let topic_b = repo
            .recent(Some("telegram"), "-100111", 10, Some("2614"), None)
            .await
            .unwrap();
        assert_eq!(
            topic_b.len(),
            1,
            "thread_id filter should return only topic B messages"
        );
        assert_eq!(topic_b[0].content, "topic B message");

        // With thread_id filter for nonexistent topic: empty
        let none = repo
            .recent(Some("telegram"), "-100111", 10, Some("9999"), None)
            .await
            .unwrap();
        assert!(none.is_empty(), "nonexistent thread_id should return empty");
    }
}

// --- ChannelSearchTool Tests ---

mod tool {
    use crate::brain::tools::channel_search::ChannelSearchTool;
    use crate::brain::tools::{Tool, ToolExecutionContext};
    use crate::db::Database;
    use crate::db::models::ChannelMessage;
    use crate::db::repository::channel_message::ChannelMessageRepository;

    async fn setup() -> (Database, ChannelMessageRepository, ChannelSearchTool) {
        let db = Database::connect_in_memory()
            .await
            .expect("Failed to create database");
        db.run_migrations().await.expect("Failed to run migrations");
        let repo = ChannelMessageRepository::new(db.pool().clone());
        let tool = ChannelSearchTool::new(repo.clone());
        (db, repo, tool)
    }

    fn ctx() -> ToolExecutionContext {
        ToolExecutionContext::new(uuid::Uuid::new_v4())
    }

    fn insert_msg(
        channel: &str,
        chat_id: &str,
        chat_name: &str,
        sender: &str,
        content: &str,
    ) -> ChannelMessage {
        ChannelMessage::new(
            channel.into(),
            chat_id.into(),
            Some(chat_name.into()),
            "u1".into(),
            sender.into(),
            content.into(),
            "text".into(),
            None,
        )
    }

    #[test]
    fn test_tool_name_and_schema() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (_db, _repo, tool) = setup().await;
            assert_eq!(tool.name(), "channel_search");
            let schema = tool.input_schema();
            let props = schema["properties"].as_object().unwrap();
            assert!(props.contains_key("operation"));
            assert!(props.contains_key("channel"));
            assert!(props.contains_key("chat_id"));
            assert!(props.contains_key("query"));
            assert!(props.contains_key("n"));
            assert!(
                props["channel"]["enum"]
                    .as_array()
                    .expect("channel enum")
                    .iter()
                    .any(|value| value == "telegram-userbot")
            );
            assert!(!tool.requires_approval());
        });
    }

    #[tokio::test]
    async fn test_list_chats_empty() {
        let (_db, _repo, tool) = setup().await;
        let input = serde_json::json!({"operation": "list_chats"});
        let result = tool.execute(input, &ctx()).await.unwrap();
        assert!(result.success);
        assert!(result.output.contains("No channel messages captured"));
    }

    #[tokio::test]
    async fn test_list_chats_with_data() {
        let (_db, repo, tool) = setup().await;
        let m1 = insert_msg("telegram", "-100111", "Dev Group", "Alice", "hello");
        let m2 = insert_msg("telegram", "-100222", "Ops Group", "Bob", "world");
        repo.insert(&m1).await.unwrap();
        repo.insert(&m2).await.unwrap();

        let input = serde_json::json!({"operation": "list_chats"});
        let result = tool.execute(input, &ctx()).await.unwrap();
        assert!(result.success);
        assert!(result.output.contains("Known chats (2)"));
        assert!(result.output.contains("Dev Group"));
        assert!(result.output.contains("Ops Group"));
    }

    #[tokio::test]
    async fn test_list_chats_filtered_by_channel() {
        let (_db, repo, tool) = setup().await;
        let m1 = insert_msg("telegram", "-100111", "TG Group", "Alice", "tg");
        let m2 = insert_msg("discord", "DC001", "DC Chan", "Bob", "dc");
        repo.insert(&m1).await.unwrap();
        repo.insert(&m2).await.unwrap();

        let input = serde_json::json!({"operation": "list_chats", "channel": "telegram"});
        let result = tool.execute(input, &ctx()).await.unwrap();
        assert!(result.success);
        assert!(result.output.contains("Known chats (1)"));
        assert!(result.output.contains("TG Group"));
        assert!(!result.output.contains("DC Chan"));
    }

    #[tokio::test]
    async fn test_recent_requires_chat_id() {
        let (_db, _repo, tool) = setup().await;
        let input = serde_json::json!({"operation": "recent"});
        let result = tool.execute(input, &ctx()).await.unwrap();
        assert!(!result.success);
        let msg = result.error.as_deref().unwrap_or(&result.output);
        assert!(msg.contains("chat_id"));
    }

    #[tokio::test]
    async fn test_recent_returns_messages() {
        let (_db, repo, tool) = setup().await;
        let m1 = insert_msg("telegram", "-100111", "Group", "Alice", "first message");
        let m2 = insert_msg("telegram", "-100111", "Group", "Bob", "second message");
        repo.insert(&m1).await.unwrap();
        repo.insert(&m2).await.unwrap();

        let input = serde_json::json!({"operation": "recent", "chat_id": "-100111"});
        let result = tool.execute(input, &ctx()).await.unwrap();
        assert!(result.success);
        assert!(result.output.contains("first message"));
        assert!(result.output.contains("second message"));
        assert!(result.output.contains("Alice"));
        assert!(result.output.contains("Bob"));
    }

    #[tokio::test]
    async fn test_recent_empty_chat() {
        let (_db, _repo, tool) = setup().await;
        let input = serde_json::json!({"operation": "recent", "chat_id": "-999"});
        let result = tool.execute(input, &ctx()).await.unwrap();
        assert!(result.success);
        assert!(result.output.contains("No messages found"));
    }

    #[tokio::test]
    async fn test_recent_with_n_limit() {
        let (_db, repo, tool) = setup().await;
        for i in 0..10 {
            let m = insert_msg("telegram", "-100111", "Group", "Alice", &format!("msg {i}"));
            repo.insert(&m).await.unwrap();
        }

        let input = serde_json::json!({"operation": "recent", "chat_id": "-100111", "n": 3});
        let result = tool.execute(input, &ctx()).await.unwrap();
        assert!(result.success);
        assert!(result.output.contains("(3)"));
    }

    #[tokio::test]
    async fn test_search_requires_query() {
        let (_db, _repo, tool) = setup().await;
        let input = serde_json::json!({"operation": "search"});
        let result = tool.execute(input, &ctx()).await.unwrap();
        assert!(!result.success);
        let msg = result.error.as_deref().unwrap_or(&result.output);
        assert!(msg.contains("query"));
    }

    #[tokio::test]
    async fn test_search_finds_messages() {
        let (_db, repo, tool) = setup().await;
        let m1 = insert_msg(
            "telegram",
            "-100111",
            "Group",
            "Alice",
            "deploy failed on prod",
        );
        let m2 = insert_msg("telegram", "-100111", "Group", "Bob", "checking logs now");
        let m3 = insert_msg(
            "slack",
            "C999",
            "General",
            "Carol",
            "deploy succeeded on staging",
        );
        repo.insert(&m1).await.unwrap();
        repo.insert(&m2).await.unwrap();
        repo.insert(&m3).await.unwrap();

        let input = serde_json::json!({"operation": "search", "query": "deploy"});
        let result = tool.execute(input, &ctx()).await.unwrap();
        assert!(result.success);
        assert!(result.output.contains("(2)")); // 2 results
        assert!(result.output.contains("Alice"));
        assert!(result.output.contains("Carol"));
    }

    #[tokio::test]
    async fn test_search_with_channel_filter() {
        let (_db, repo, tool) = setup().await;
        let m1 = insert_msg("telegram", "-100111", "Group", "Alice", "error happened");
        let m2 = insert_msg("slack", "C999", "General", "Bob", "error resolved");
        repo.insert(&m1).await.unwrap();
        repo.insert(&m2).await.unwrap();

        let input =
            serde_json::json!({"operation": "search", "query": "error", "channel": "telegram"});
        let result = tool.execute(input, &ctx()).await.unwrap();
        assert!(result.success);
        assert!(result.output.contains("(1)"));
        assert!(result.output.contains("Alice"));
        assert!(!result.output.contains("Bob"));
    }

    #[tokio::test]
    async fn test_search_no_match() {
        let (_db, repo, tool) = setup().await;
        let m = insert_msg("telegram", "-100111", "Group", "Alice", "hello");
        repo.insert(&m).await.unwrap();

        let input = serde_json::json!({"operation": "search", "query": "nonexistent"});
        let result = tool.execute(input, &ctx()).await.unwrap();
        assert!(result.success);
        assert!(result.output.contains("No messages matching"));
    }

    #[tokio::test]
    async fn test_unknown_operation() {
        let (_db, _repo, tool) = setup().await;
        let input = serde_json::json!({"operation": "invalid"});
        let result = tool.execute(input, &ctx()).await.unwrap();
        assert!(!result.success);
        let msg = result.error.as_deref().unwrap_or(&result.output);
        assert!(msg.contains("Unknown operation"));
    }

    fn insert_msg_with_pmid(
        channel: &str,
        chat_id: &str,
        chat_name: &str,
        sender: &str,
        content: &str,
        platform_message_id: &str,
    ) -> ChannelMessage {
        ChannelMessage::new(
            channel.into(),
            chat_id.into(),
            Some(chat_name.into()),
            "u1".into(),
            sender.into(),
            content.into(),
            "text".into(),
            Some(platform_message_id.into()),
        )
    }

    #[tokio::test]
    async fn test_recent_includes_platform_message_id() {
        let (_db, repo, tool) = setup().await;
        let m = insert_msg_with_pmid(
            "telegram",
            "-100111",
            "OC Dev",
            "Alice",
            "hello world",
            "6895",
        );
        repo.insert(&m).await.unwrap();

        let input = serde_json::json!({"operation": "recent", "chat_id": "-100111"});
        let result = tool.execute(input, &ctx()).await.unwrap();
        assert!(result.success);
        assert!(
            result.output.contains("[msgid:6895]"),
            "recent output should contain [msgid:6895], got: {}",
            result.output
        );
        assert!(result.output.contains("Alice"));
        assert!(result.output.contains("hello world"));
    }

    #[tokio::test]
    async fn test_recent_omits_msgid_when_none() {
        let (_db, repo, tool) = setup().await;
        let m = insert_msg("telegram", "-100111", "Group", "Bob", "no id here");
        repo.insert(&m).await.unwrap();

        let input = serde_json::json!({"operation": "recent", "chat_id": "-100111"});
        let result = tool.execute(input, &ctx()).await.unwrap();
        assert!(result.success);
        assert!(
            !result.output.contains("[msgid:"),
            "recent output should NOT contain [msgid:] when platform_message_id is None, got: {}",
            result.output
        );
    }

    #[tokio::test]
    async fn test_search_includes_platform_message_id() {
        let (_db, repo, tool) = setup().await;
        let m1 = insert_msg_with_pmid(
            "telegram",
            "-100111",
            "OC Dev",
            "Alice",
            "deploy failed",
            "ts_42",
        );
        let m2 = insert_msg("slack", "C999", "General", "Carol", "deploy succeeded");
        repo.insert(&m1).await.unwrap();
        repo.insert(&m2).await.unwrap();

        let input = serde_json::json!({"operation": "search", "query": "deploy"});
        let result = tool.execute(input, &ctx()).await.unwrap();
        assert!(result.success);
        assert!(
            result.output.contains("[msgid:ts_42]"),
            "search output should contain [msgid:ts_42], got: {}",
            result.output
        );
        // Carol's message has no pmid so no [msgid:] for her
        assert!(
            !result.output.contains("[msgid:] "),
            "should not have empty [msgid:]"
        );
    }

    #[tokio::test]
    async fn test_search_mixed_pmid_and_none() {
        let (_db, repo, tool) = setup().await;
        let m1 = insert_msg_with_pmid(
            "telegram",
            "-100111",
            "Group",
            "Alice",
            "first msg",
            "msg_100",
        );
        let m2 = insert_msg("telegram", "-100111", "Group", "Bob", "second msg");
        repo.insert(&m1).await.unwrap();
        repo.insert(&m2).await.unwrap();

        let input = serde_json::json!({"operation": "search", "query": "msg"});
        let result = tool.execute(input, &ctx()).await.unwrap();
        assert!(result.success);
        assert!(
            result.output.contains("[msgid:msg_100]"),
            "Alice's message should have [msgid:msg_100], got: {}",
            result.output
        );
        // Bob's line should not have [msgid:]
        let lines: Vec<&str> = result.output.lines().collect();
        let bob_line = lines.iter().find(|l| l.contains("Bob")).unwrap();
        assert!(
            !bob_line.contains("[msgid:"),
            "Bob's line should not have [msgid:], got: {}",
            bob_line
        );
    }
}
