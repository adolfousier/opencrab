-- Session seen-skills persistence (issue #138): the in-memory seen_skills
-- registry dies on every daemon restart, so a session that consumed a skill
-- before a rebuild looks "skill-less" to the post-compaction inventory stamp
-- (#125/#131). A row here is written on every mark_seen; boot hydrates the
-- in-memory registry from this table so stamps survive restarts.
CREATE TABLE IF NOT EXISTS session_seen_skills (
    session_id TEXT NOT NULL,
    slug       TEXT NOT NULL,
    seen_at    INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    PRIMARY KEY (session_id, slug)
);

CREATE INDEX IF NOT EXISTS idx_session_seen_skills_session
    ON session_seen_skills(session_id);
