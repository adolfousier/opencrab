-- Goal state table: persists autonomous goal tracking per session.
-- A goal is a free-form objective that persists across turns. After each
-- turn completes, a judge call evaluates whether the goal is satisfied.
CREATE TABLE IF NOT EXISTS goal_state (
    id         TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL,
    goal_text  TEXT NOT NULL,
    state      TEXT NOT NULL DEFAULT 'active'
        CHECK (state IN ('active', 'paused', 'completed', 'failed')),
    turns_used                   INTEGER NOT NULL DEFAULT 0,
    max_turns                    INTEGER NOT NULL DEFAULT 20,
    consecutive_parse_failures   INTEGER NOT NULL DEFAULT 0,
    judge_verdict                TEXT,
    judge_reason                 TEXT,
    channel                      TEXT,
    channel_chat_id              TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_goal_state_session ON goal_state(session_id);
CREATE INDEX IF NOT EXISTS idx_goal_state_active ON goal_state(session_id, state);
