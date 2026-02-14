-- Initial database schema for OpenCrabs
-- SQLite database for local-first AI assistant

-- Sessions table: Stores chat sessions
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    model TEXT NOT NULL,
    provider TEXT NOT NULL,
    created_at INTEGER NOT NULL,  -- Unix timestamp
    updated_at INTEGER NOT NULL,  -- Unix timestamp
    total_tokens INTEGER NOT NULL DEFAULT 0,
    total_cost REAL NOT NULL DEFAULT 0.0,
    message_count INTEGER NOT NULL DEFAULT 0,
    is_archived INTEGER NOT NULL DEFAULT 0  -- Boolean: 0=false, 1=true
);

-- Messages table: Stores individual messages in sessions
CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL,
    role TEXT NOT NULL,  -- 'user', 'assistant', 'system', 'tool'
    content TEXT NOT NULL,
    created_at INTEGER NOT NULL,  -- Unix timestamp
    input_tokens INTEGER,
    output_tokens INTEGER,
    cost REAL,
    reasoning_tokens INTEGER,  -- For reasoning models
    cache_creation_tokens INTEGER,  -- For prompt caching
    cache_read_tokens INTEGER,

    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

-- Files table: Stores file metadata and read/write history
CREATE TABLE IF NOT EXISTS files (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL,
    path TEXT NOT NULL,
    operation TEXT NOT NULL,  -- 'read', 'write', 'edit'
    content_hash TEXT,  -- SHA-256 hash of content
    size_bytes INTEGER,
    created_at INTEGER NOT NULL,  -- Unix timestamp

    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

-- Message attachments: For image/file attachments in messages
CREATE TABLE IF NOT EXISTS attachments (
    id TEXT PRIMARY KEY NOT NULL,
    message_id TEXT NOT NULL,
    type TEXT NOT NULL,  -- 'image', 'file', 'text'
    mime_type TEXT,
    path TEXT,
    size_bytes INTEGER,
    created_at INTEGER NOT NULL,

    FOREIGN KEY (message_id) REFERENCES messages(id) ON DELETE CASCADE
);

-- Tool executions: Track tool usage and permissions
CREATE TABLE IF NOT EXISTS tool_executions (
    id TEXT PRIMARY KEY NOT NULL,
    message_id TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    arguments TEXT NOT NULL,  -- JSON
    result TEXT,  -- JSON
    status TEXT NOT NULL,  -- 'pending', 'approved', 'denied', 'executed', 'failed'
    approved_at INTEGER,
    executed_at INTEGER,
    created_at INTEGER NOT NULL,

    FOREIGN KEY (message_id) REFERENCES messages(id) ON DELETE CASCADE
);

-- Indexes for common queries
CREATE INDEX IF NOT EXISTS idx_sessions_updated_at ON sessions(updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_sessions_archived ON sessions(is_archived, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_messages_session_id ON messages(session_id, created_at ASC);
CREATE INDEX IF NOT EXISTS idx_files_session_id ON files(session_id);
CREATE INDEX IF NOT EXISTS idx_files_path ON files(path);
CREATE INDEX IF NOT EXISTS idx_attachments_message_id ON attachments(message_id);
CREATE INDEX IF NOT EXISTS idx_tool_executions_message_id ON tool_executions(message_id);
CREATE INDEX IF NOT EXISTS idx_tool_executions_status ON tool_executions(status);
