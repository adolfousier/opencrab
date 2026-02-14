-- Migration to add Plan Mode support
-- Adds tables for storing plans and their tasks

-- ==================================================
-- Plans Table
-- ==================================================

CREATE TABLE IF NOT EXISTS plans (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    context TEXT NOT NULL DEFAULT '',
    risks TEXT NOT NULL DEFAULT '[]',  -- JSON array of strings
    status TEXT NOT NULL,  -- Draft, PendingApproval, Approved, Rejected, InProgress, Completed, Cancelled
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    approved_at INTEGER,  -- Timestamp when approved

    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

-- ==================================================
-- Plan Tasks Table
-- ==================================================

CREATE TABLE IF NOT EXISTS plan_tasks (
    id TEXT PRIMARY KEY NOT NULL,
    plan_id TEXT NOT NULL,
    task_order INTEGER NOT NULL,  -- Order/sequence number
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    task_type TEXT NOT NULL,  -- Research, Edit, Create, Delete, Test, Refactor, Documentation, Configuration, Build, Other
    dependencies TEXT NOT NULL DEFAULT '[]',  -- JSON array of task IDs
    complexity INTEGER NOT NULL DEFAULT 3,  -- 1-5 scale
    status TEXT NOT NULL,  -- Pending, InProgress, Completed, Skipped, Failed, Blocked
    notes TEXT,  -- Execution notes/results
    completed_at INTEGER,  -- Timestamp when completed

    FOREIGN KEY (plan_id) REFERENCES plans(id) ON DELETE CASCADE
);

-- ==================================================
-- Indexes
-- ==================================================

CREATE INDEX IF NOT EXISTS idx_plans_session_id ON plans(session_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_plans_status ON plans(status);
CREATE INDEX IF NOT EXISTS idx_plan_tasks_plan_id ON plan_tasks(plan_id, task_order ASC);
CREATE INDEX IF NOT EXISTS idx_plan_tasks_status ON plan_tasks(status);
