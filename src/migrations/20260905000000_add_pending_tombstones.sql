-- Pending tombstone table: durable parking for sub-agent death reports.
--
-- A sub-agent dies with the process (#73), and its death report is produced
-- at the NEXT startup by reconciliation. If no session route has claimed the
-- parent by then, the report is parked in memory — and the very class of
-- restart that killed the agent also wipes that memory, silently losing the
-- report while the status file has already gone terminal and will never be
-- re-reported.
--
-- A row here exists only while the report is still undelivered. Startup
-- re-offers every surviving row before reconciliation runs; the row is
-- cleared the moment the report actually reaches a surface (route claim or
-- local flush), so a delivered report is never delivered twice by this path.
CREATE TABLE IF NOT EXISTS pending_tombstones (
    id           TEXT PRIMARY KEY NOT NULL,
    session_id   TEXT NOT NULL,
    context_text TEXT NOT NULL,
    display_text TEXT NOT NULL,
    created_at   INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_pending_tombstones_session ON pending_tombstones(session_id);
