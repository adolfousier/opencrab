-- Migration to modernize schema with improved type safety
-- Updates models to use cleaner structure with better semantics

-- ==================================================
-- Sessions Table Updates
-- ==================================================

-- Create new sessions table with updated structure
CREATE TABLE IF NOT EXISTS sessions_new (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT,  -- Made optional
    model TEXT,  -- Made optional
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    archived_at INTEGER,  -- Replaced is_archived with timestamp
    token_count INTEGER NOT NULL DEFAULT 0,  -- Renamed from total_tokens
    total_cost REAL NOT NULL DEFAULT 0.0
);

-- Copy data from old table to new table
INSERT INTO sessions_new (id, title, model, created_at, updated_at, archived_at, token_count, total_cost)
SELECT
    id,
    NULLIF(title, '') as title,  -- Convert empty strings to NULL
    NULLIF(model, '') as model,  -- Convert empty strings to NULL
    created_at,
    updated_at,
    CASE WHEN is_archived = 1 THEN updated_at ELSE NULL END as archived_at,
    total_tokens as token_count,
    total_cost
FROM sessions;

-- Drop old table and rename new table
DROP TABLE sessions;
ALTER TABLE sessions_new RENAME TO sessions;

-- ==================================================
-- Messages Table Updates
-- ==================================================

-- Create new messages table with updated structure
CREATE TABLE IF NOT EXISTS messages_new (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    sequence INTEGER NOT NULL,  -- New field for message ordering
    created_at INTEGER NOT NULL,
    token_count INTEGER,  -- Simplified from separate input/output tokens
    cost REAL,

    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

-- Copy data from old table with sequence numbers
INSERT INTO messages_new (id, session_id, role, content, sequence, created_at, token_count, cost)
SELECT
    id,
    session_id,
    role,
    content,
    ROW_NUMBER() OVER (PARTITION BY session_id ORDER BY created_at) as sequence,
    created_at,
    COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0) as token_count,
    cost
FROM messages;

-- Drop old table and rename new table
DROP TABLE messages;
ALTER TABLE messages_new RENAME TO messages;

-- ==================================================
-- Files Table Updates
-- ==================================================

-- Create new files table with updated structure
CREATE TABLE IF NOT EXISTS files_new (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL,
    path TEXT NOT NULL,
    content TEXT,  -- Optional file content
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,  -- New field

    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

-- Copy data from old table
INSERT INTO files_new (id, session_id, path, content, created_at, updated_at)
SELECT
    id,
    session_id,
    path,
    NULL as content,  -- Old table didn't store content
    created_at,
    created_at as updated_at  -- Initialize with created_at
FROM files;

-- Drop old table and rename new table
DROP TABLE files;
ALTER TABLE files_new RENAME TO files;

-- ==================================================
-- Update Indexes
-- ==================================================

-- Drop old indexes
DROP INDEX IF EXISTS idx_sessions_archived;

-- Create new indexes for updated schema
CREATE INDEX IF NOT EXISTS idx_sessions_updated_at ON sessions(updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_sessions_archived ON sessions(archived_at DESC);  -- Changed to use archived_at
CREATE INDEX IF NOT EXISTS idx_messages_session_id ON messages(session_id, sequence ASC);  -- Changed to use sequence
CREATE INDEX IF NOT EXISTS idx_files_session_id ON files(session_id);
CREATE INDEX IF NOT EXISTS idx_files_path ON files(path);
CREATE INDEX IF NOT EXISTS idx_attachments_message_id ON attachments(message_id);
CREATE INDEX IF NOT EXISTS idx_tool_executions_message_id ON tool_executions(message_id);
CREATE INDEX IF NOT EXISTS idx_tool_executions_status ON tool_executions(status);
