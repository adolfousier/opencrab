-- Durable backing for Telegram pending follow-up suggestion keyboards
-- (#1226 item 3). The token-keyed stash lived only in memory, so any
-- deploy/restart between render and tap left the rendered keyboard
-- permanently dead: taps hit the unknown-token warn with no way to tell
-- a consumed picker from a restart-eaten one. Same lifecycle as
-- plan_cards (#809): rows are written when a keyboard is armed (and when
-- its merge host attaches), deleted on tap/drop/clear, and hydrated back
-- into the in-memory map on boot so surviving keyboards keep working.

CREATE TABLE IF NOT EXISTS pending_followups (
    token           TEXT PRIMARY KEY,
    session_id      TEXT NOT NULL,
    options_json    TEXT NOT NULL,
    host_message_id INTEGER,
    host_html       TEXT,
    host_rich       INTEGER NOT NULL DEFAULT 0,
    updated_at      INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

CREATE INDEX IF NOT EXISTS idx_pending_followups_session
    ON pending_followups(session_id);
