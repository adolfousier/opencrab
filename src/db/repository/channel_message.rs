//! Channel Message Repository
//!
//! Database operations for passively captured channel messages.

use crate::db::Pool;
use crate::db::database::interact_err;
use crate::db::models::ChannelMessage;
use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, params};

/// Summary of a known chat
pub struct ChatSummary {
    pub channel: String,
    pub channel_chat_id: String,
    pub channel_chat_name: Option<String>,
    pub message_count: i64,
    pub last_message_at: i64,
}

/// Repository for channel message operations
#[derive(Clone)]
pub struct ChannelMessageRepository {
    pool: Pool,
}

impl ChannelMessageRepository {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }

    /// Insert a single channel message
    pub async fn insert(&self, msg: &ChannelMessage) -> Result<()> {
        let m = msg.clone();
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                conn.execute(
                    "INSERT OR IGNORE INTO channel_messages
                        (id, channel, channel_chat_id, channel_chat_name,
                         sender_id, sender_name, content, message_type,
                         platform_message_id, created_at, thread_id, topic_name)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    params![
                        m.id.to_string(),
                        m.channel,
                        m.channel_chat_id,
                        m.channel_chat_name,
                        m.sender_id,
                        m.sender_name,
                        m.content,
                        m.message_type,
                        m.platform_message_id,
                        m.created_at.timestamp(),
                        m.thread_id,
                        m.topic_name,
                    ],
                )
            })
            .await
            .map_err(interact_err)?
            .context("Failed to insert channel message")?;

        Ok(())
    }

    /// Rewrite a stored message's content, matched by platform message id.
    ///
    /// Reconciles streamed/edited messages from peer bots: a bot that
    /// streams its reply via Telegram message edits arrives as one initial
    /// frame plus a run of `edited_message` updates. When an edit lands
    /// after we already captured the message, this rewrites the row so
    /// group history reflects the FINAL text rather than the first frame.
    /// Returns the number of rows updated (0 if the message wasn't stored).
    pub async fn update_content(
        &self,
        channel: &str,
        chat_id: &str,
        platform_message_id: &str,
        content: &str,
    ) -> Result<usize> {
        let ch = channel.to_string();
        let cid = chat_id.to_string();
        let pmid = platform_message_id.to_string();
        let body = content.to_string();
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                conn.execute(
                    "UPDATE channel_messages SET content = ?4 \
                     WHERE channel = ?1 AND channel_chat_id = ?2 \
                       AND platform_message_id = ?3",
                    params![ch, cid, pmid, body],
                )
            })
            .await
            .map_err(interact_err)?
            .context("Failed to update channel message content")
    }

    /// Get recent messages for a specific chat, optionally filtered by thread_id.
    /// When `thread_id` is Some, only messages belonging to that forum topic are returned.
    pub async fn recent(
        &self,
        channel: Option<&str>,
        chat_id: &str,
        limit: i64,
        thread_id: Option<&str>,
    ) -> Result<Vec<ChannelMessage>> {
        let ch = channel.map(|s| s.to_string());
        let cid = chat_id.to_string();
        let tid = thread_id.map(|s| s.to_string());
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| match (ch, tid) {
                (Some(ch), Some(tid)) => {
                    let mut stmt = conn.prepare_cached(
                        "SELECT * FROM channel_messages \
                             WHERE channel = ?1 AND channel_chat_id = ?2 AND thread_id = ?3 \
                             ORDER BY created_at DESC LIMIT ?4",
                    )?;
                    let rows =
                        stmt.query_map(params![ch, cid, tid, limit], ChannelMessage::from_row)?;
                    rows.collect::<std::result::Result<Vec<_>, _>>()
                }
                (Some(ch), None) => {
                    let mut stmt = conn.prepare_cached(
                        "SELECT * FROM channel_messages \
                             WHERE channel = ?1 AND channel_chat_id = ?2 \
                             ORDER BY created_at DESC LIMIT ?3",
                    )?;
                    let rows = stmt.query_map(params![ch, cid, limit], ChannelMessage::from_row)?;
                    rows.collect::<std::result::Result<Vec<_>, _>>()
                }
                (None, Some(tid)) => {
                    let mut stmt = conn.prepare_cached(
                        "SELECT * FROM channel_messages \
                             WHERE channel_chat_id = ?1 AND thread_id = ?2 \
                             ORDER BY created_at DESC LIMIT ?3",
                    )?;
                    let rows =
                        stmt.query_map(params![cid, tid, limit], ChannelMessage::from_row)?;
                    rows.collect::<std::result::Result<Vec<_>, _>>()
                }
                (None, None) => {
                    let mut stmt = conn.prepare_cached(
                        "SELECT * FROM channel_messages \
                             WHERE channel_chat_id = ?1 \
                             ORDER BY created_at DESC LIMIT ?2",
                    )?;
                    let rows = stmt.query_map(params![cid, limit], ChannelMessage::from_row)?;
                    rows.collect::<std::result::Result<Vec<_>, _>>()
                }
            })
            .await
            .map_err(interact_err)?
            .context("Failed to fetch recent channel messages")
    }

    /// Look up the stored content of a single message by its platform message id
    /// (the Telegram `message_id`). Used to recover the EXACT message a user
    /// replied to — Telegram delivers rich bot messages with empty text, so the
    /// reply handler can't read the quoted content from the update and must look
    /// it up by id instead of guessing "the most recent bot message". Returns
    /// the most recent match (ids are unique per chat, but a defensive ORDER BY
    /// keeps this deterministic). `None` when nothing was stored under that id.
    pub async fn content_by_platform_message_id(
        &self,
        channel: &str,
        chat_id: &str,
        platform_message_id: &str,
    ) -> Result<Option<String>> {
        let ch = channel.to_string();
        let cid = chat_id.to_string();
        let pmid = platform_message_id.to_string();
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                conn.query_row(
                    "SELECT content FROM channel_messages \
                     WHERE channel = ?1 AND channel_chat_id = ?2 AND platform_message_id = ?3 \
                     ORDER BY created_at DESC LIMIT 1",
                    params![ch, cid, pmid],
                    |row| row.get::<_, String>(0),
                )
                .optional()
            })
            .await
            .map_err(interact_err)?
            .context("Failed to look up channel message by platform id")
    }

    /// Most recent non-null forum topic name for a thread, used to label the
    /// session after the topic ("Devops") instead of its numeric thread id
    /// ("topic:2"). Telegram only carries the name on regular topic messages
    /// (via the topic-creation reply target); a user replying to a specific
    /// message inside the topic omits it, so we read the last one we persisted
    /// to keep the label stable.
    pub async fn latest_topic_name(
        &self,
        channel: &str,
        chat_id: &str,
        thread_id: &str,
    ) -> Result<Option<String>> {
        let ch = channel.to_string();
        let cid = chat_id.to_string();
        let tid = thread_id.to_string();
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                conn.query_row(
                    "SELECT topic_name FROM channel_messages \
                         WHERE channel = ?1 AND channel_chat_id = ?2 AND thread_id = ?3 \
                         AND topic_name IS NOT NULL AND topic_name != '' \
                         ORDER BY created_at DESC LIMIT 1",
                    params![ch, cid, tid],
                    |row| row.get::<_, String>(0),
                )
                .optional()
            })
            .await
            .map_err(interact_err)?
            .context("Failed to fetch latest topic name")
    }

    /// Search messages by content (LIKE-based) with optional thread_id filter
    pub async fn search(
        &self,
        channel: Option<&str>,
        chat_id: Option<&str>,
        query: &str,
        limit: i64,
        thread_id: Option<&str>,
    ) -> Result<Vec<ChannelMessage>> {
        let ch = channel.map(|s| s.to_string());
        let cid = chat_id.map(|s| s.to_string());
        let tid = thread_id.map(|s| s.to_string());
        let pattern = format!("%{query}%");

        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| match (ch, cid, tid) {
                (Some(ch), Some(cid), Some(tid)) => {
                    let mut stmt = conn.prepare_cached(
                        "SELECT * FROM channel_messages \
                             WHERE channel = ?1 AND channel_chat_id = ?2 AND content LIKE ?3 AND thread_id = ?4 \
                             ORDER BY created_at DESC LIMIT ?5",
                    )?;
                    let rows =
                        stmt.query_map(params![ch, cid, pattern, tid, limit], ChannelMessage::from_row)?;
                    rows.collect::<std::result::Result<Vec<_>, _>>()
                }
                (Some(ch), Some(cid), None) => {
                    let mut stmt = conn.prepare_cached(
                        "SELECT * FROM channel_messages \
                             WHERE channel = ?1 AND channel_chat_id = ?2 AND content LIKE ?3 \
                             ORDER BY created_at DESC LIMIT ?4",
                    )?;
                    let rows =
                        stmt.query_map(params![ch, cid, pattern, limit], ChannelMessage::from_row)?;
                    rows.collect::<std::result::Result<Vec<_>, _>>()
                }
                (Some(ch), None, Some(tid)) => {
                    let mut stmt = conn.prepare_cached(
                        "SELECT * FROM channel_messages \
                             WHERE channel = ?1 AND content LIKE ?2 AND thread_id = ?3 \
                             ORDER BY created_at DESC LIMIT ?4",
                    )?;
                    let rows =
                        stmt.query_map(params![ch, pattern, tid, limit], ChannelMessage::from_row)?;
                    rows.collect::<std::result::Result<Vec<_>, _>>()
                }
                (Some(ch), None, None) => {
                    let mut stmt = conn.prepare_cached(
                        "SELECT * FROM channel_messages \
                             WHERE channel = ?1 AND content LIKE ?2 \
                             ORDER BY created_at DESC LIMIT ?3",
                    )?;
                    let rows =
                        stmt.query_map(params![ch, pattern, limit], ChannelMessage::from_row)?;
                    rows.collect::<std::result::Result<Vec<_>, _>>()
                }
                (None, Some(cid), Some(tid)) => {
                    let mut stmt = conn.prepare_cached(
                        "SELECT * FROM channel_messages \
                             WHERE channel_chat_id = ?1 AND content LIKE ?2 AND thread_id = ?3 \
                             ORDER BY created_at DESC LIMIT ?4",
                    )?;
                    let rows =
                        stmt.query_map(params![cid, pattern, tid, limit], ChannelMessage::from_row)?;
                    rows.collect::<std::result::Result<Vec<_>, _>>()
                }
                (None, Some(cid), None) => {
                    let mut stmt = conn.prepare_cached(
                        "SELECT * FROM channel_messages \
                             WHERE channel_chat_id = ?1 AND content LIKE ?2 \
                             ORDER BY created_at DESC LIMIT ?3",
                    )?;
                    let rows =
                        stmt.query_map(params![cid, pattern, limit], ChannelMessage::from_row)?;
                    rows.collect::<std::result::Result<Vec<_>, _>>()
                }
                (None, None, Some(tid)) => {
                    let mut stmt = conn.prepare_cached(
                        "SELECT * FROM channel_messages \
                             WHERE content LIKE ?1 AND thread_id = ?2 \
                             ORDER BY created_at DESC LIMIT ?3",
                    )?;
                    let rows =
                        stmt.query_map(params![pattern, tid, limit], ChannelMessage::from_row)?;
                    rows.collect::<std::result::Result<Vec<_>, _>>()
                }
                (None, None, None) => {
                    let mut stmt = conn.prepare_cached(
                        "SELECT * FROM channel_messages \
                             WHERE content LIKE ?1 \
                             ORDER BY created_at DESC LIMIT ?2",
                    )?;
                    let rows = stmt.query_map(params![pattern, limit], ChannelMessage::from_row)?;
                    rows.collect::<std::result::Result<Vec<_>, _>>()
                }
            })
            .await
            .map_err(interact_err)?
            .context("Failed to search channel messages")
    }

    /// List distinct chats with message count and last message time
    pub async fn list_chats(&self, channel: Option<&str>) -> Result<Vec<ChatSummary>> {
        let ch = channel.map(|s| s.to_string());
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                if let Some(ch) = ch {
                    let mut stmt = conn.prepare_cached(
                        "SELECT channel, channel_chat_id, \
                                MAX(channel_chat_name) as channel_chat_name, \
                                COUNT(*) as message_count, \
                                MAX(created_at) as last_message_at \
                         FROM channel_messages \
                         WHERE channel = ?1 \
                         GROUP BY channel, channel_chat_id \
                         ORDER BY last_message_at DESC",
                    )?;
                    let rows = stmt.query_map(params![ch], |row| {
                        Ok(ChatSummary {
                            channel: row.get(0)?,
                            channel_chat_id: row.get(1)?,
                            channel_chat_name: row.get(2)?,
                            message_count: row.get(3)?,
                            last_message_at: row.get(4)?,
                        })
                    })?;
                    rows.collect::<std::result::Result<Vec<_>, _>>()
                } else {
                    let mut stmt = conn.prepare_cached(
                        "SELECT channel, channel_chat_id, \
                                MAX(channel_chat_name) as channel_chat_name, \
                                COUNT(*) as message_count, \
                                MAX(created_at) as last_message_at \
                         FROM channel_messages \
                         GROUP BY channel, channel_chat_id \
                         ORDER BY last_message_at DESC",
                    )?;
                    let rows = stmt.query_map([], |row| {
                        Ok(ChatSummary {
                            channel: row.get(0)?,
                            channel_chat_id: row.get(1)?,
                            channel_chat_name: row.get(2)?,
                            message_count: row.get(3)?,
                            last_message_at: row.get(4)?,
                        })
                    })?;
                    rows.collect::<std::result::Result<Vec<_>, _>>()
                }
            })
            .await
            .map_err(interact_err)?
            .context("Failed to list channel chats")
    }

    /// Return all distinct forum topics seen in a chat — the
    /// (thread_id, topic_name) pairs captured from incoming messages.
    ///
    /// The bot only learns topic names from two sources:
    ///
    /// - `forum_topic_created` service messages it witnesses
    /// - the `reply_to_message().forum_topic_created()` chain that
    ///   Telegram includes on every regular topic message
    ///
    /// So this list only contains topics the bot has seen activity
    /// in. Telegram's Bot API exposes no `listForumTopics` endpoint
    /// — there is no way to enumerate all topics in a supergroup,
    /// only learn them passively as messages arrive.
    ///
    /// Used by `telegram_send`'s `list_topics` action so the agent
    /// can map user-typed names like "#announcements" back to the
    /// numeric `thread_id` it needs to pass to `message_in_thread`.
    pub async fn topics_for_chat(&self, channel: &str, chat_id: &str) -> Result<Vec<TopicSummary>> {
        let ch = channel.to_string();
        let cid = chat_id.to_string();
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                let mut stmt = conn.prepare_cached(
                    "SELECT thread_id, \
                            MAX(topic_name) as topic_name, \
                            COUNT(*) as message_count, \
                            MAX(created_at) as last_message_at \
                     FROM channel_messages \
                     WHERE channel = ?1 AND channel_chat_id = ?2 AND thread_id IS NOT NULL \
                     GROUP BY thread_id \
                     ORDER BY last_message_at DESC",
                )?;
                let rows = stmt.query_map(params![ch, cid], |row| {
                    Ok(TopicSummary {
                        thread_id: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                        topic_name: row.get(1)?,
                        message_count: row.get(2)?,
                        last_message_at: row.get(3)?,
                    })
                })?;
                rows.collect::<std::result::Result<Vec<_>, _>>()
            })
            .await
            .map_err(interact_err)?
            .context("Failed to list topics for chat")
    }
}

/// A forum topic the bot has seen activity in. Surfaced by
/// `ChannelMessageRepository::topics_for_chat` so the agent can
/// translate user-typed topic names to numeric thread_ids.
#[derive(Debug, Clone)]
pub struct TopicSummary {
    pub thread_id: String,
    /// `None` for topics where we only ever saw messages but never
    /// the topic-creation service message and never a reply chain
    /// carrying it. The thread_id is still usable for sending.
    pub topic_name: Option<String>,
    pub message_count: i64,
    /// Unix epoch seconds — matches the column type
    /// `created_at INTEGER` used by every other query in this file.
    pub last_message_at: i64,
}
