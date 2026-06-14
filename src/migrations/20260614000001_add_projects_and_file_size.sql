-- Projects and file tracking enhancements
-- Adds projects table for grouping sessions, file size tracking,
-- and project_id foreign key on sessions.

-- ==================================================
-- Projects Table
-- ==================================================

CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_projects_name ON projects(name);

-- ==================================================
-- Sessions: add project_id FK
-- ==================================================

ALTER TABLE sessions ADD COLUMN project_id TEXT REFERENCES projects(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_sessions_project_id ON sessions(project_id);

-- ==================================================
-- Files: add size column (nullable, backcompat)
-- ==================================================

ALTER TABLE files ADD COLUMN size INTEGER;
