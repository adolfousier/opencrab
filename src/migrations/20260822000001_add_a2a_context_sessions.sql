-- A2A conversation continuity (#1159): maps an A2A context_id to the chat
-- session created for its first task, so follow-up tasks sharing the same
-- context_id continue that thread instead of forking a fresh session.
-- Stale rows self-heal: the lookup JOINs against sessions, so a deleted or
-- archived session simply yields no row and a new one is created.

CREATE TABLE IF NOT EXISTS a2a_context_sessions (
    context_id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL,
    updated_at INTEGER NOT NULL               -- Unix timestamp
);
