-- Analytics event tables for Mission Control phantom/provider/model tracking (#897)

CREATE TABLE IF NOT EXISTS phantom_events (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL DEFAULT '',
    provider TEXT,
    model TEXT,
    detected_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    resolved INTEGER NOT NULL DEFAULT 0,
    retry_count INTEGER NOT NULL DEFAULT 0,
    tools_after_retry INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_phantom_events_detected_at ON phantom_events(detected_at);
CREATE INDEX IF NOT EXISTS idx_phantom_events_session ON phantom_events(session_id);

CREATE TABLE IF NOT EXISTS streaming_recoveries (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL DEFAULT '',
    provider TEXT,
    model TEXT,
    recovered_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    tool_count INTEGER NOT NULL DEFAULT 1
);

CREATE INDEX IF NOT EXISTS idx_streaming_recoveries_at ON streaming_recoveries(recovered_at);

CREATE TABLE IF NOT EXISTS brain_verify_events (
    id TEXT PRIMARY KEY,
    file_name TEXT NOT NULL,
    event_type TEXT NOT NULL DEFAULT 'pass',
    violations TEXT,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_brain_verify_events_at ON brain_verify_events(created_at);

-- Extend tool_executions with provider/model/duration for per-model analytics
ALTER TABLE tool_executions ADD COLUMN provider TEXT;
ALTER TABLE tool_executions ADD COLUMN model TEXT;
ALTER TABLE tool_executions ADD COLUMN duration_ms INTEGER;
