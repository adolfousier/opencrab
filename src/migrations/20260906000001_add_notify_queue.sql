-- Durable notify queue (#111): session_notify / background-task pushes that
-- could not be delivered are parked in memory today (awaiting_channel_route
-- / PARKED vec) — a restart wipes them while the sender has already been
-- told "queued". A row here exists only while the push is still undelivered:
-- it is recorded when a push parks, consumed the moment delivery succeeds,
-- and re-offered at boot so a push parked at kill time still reaches its
-- session on the next start.
CREATE TABLE IF NOT EXISTS notify_queue (
    id           TEXT PRIMARY KEY NOT NULL,
    session_id   TEXT NOT NULL,
    context_text TEXT NOT NULL,
    display_text TEXT NOT NULL,
    origin       TEXT NOT NULL,
    bg_meta      TEXT,
    created_at   INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_notify_queue_session ON notify_queue(session_id);
