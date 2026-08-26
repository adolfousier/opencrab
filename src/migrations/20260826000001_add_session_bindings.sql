-- Persisted session↔channel bindings (#1224).
--
-- Ingress records where each session lives so a restart can re-register its
-- delivery route at channel-connect time. Without this row a session that
-- was idle at boot stays unclaimed: every background-task completion and
-- sub-agent result parks indefinitely until a human messages the session's
-- topic again and the ingress handler happens to claim it.
CREATE TABLE IF NOT EXISTS session_bindings (
    session_id TEXT PRIMARY KEY,
    channel TEXT NOT NULL,
    chat_id TEXT NOT NULL,
    thread_id INTEGER,
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_session_bindings_channel
    ON session_bindings(channel, updated_at);
