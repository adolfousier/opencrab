-- Background task table: survives a restart so an in-flight detached command
-- is not silently lost.
--
-- A genuinely long command (cargo test, a build) runs detached and the turn
-- ends immediately, on the promise that the session resumes when it finishes.
-- That promise lived entirely in an in-memory map, so killing the process
-- dropped the child AND the record of it: the session simply never continued
-- and nothing said why (#763).
--
-- A row exists only while the command is believed to be running. Startup
-- treats every surviving row as interrupted, since the process that owned the
-- child is gone, and clears it after telling the session.
CREATE TABLE IF NOT EXISTS background_tasks (
    id         TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL,
    label      TEXT NOT NULL,
    command    TEXT NOT NULL,
    cwd        TEXT NOT NULL,
    started_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_background_tasks_session ON background_tasks(session_id);
