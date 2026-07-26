-- Plan-card tracking: survives a restart so the card in the chat stays live.
--
-- The Telegram plan card (#580) is a message edited in place as tasks tick
-- over. Which message that is lived only in an in-memory map, so any restart
-- wiped it. From then on the card could not be edited (no tracked id) and
-- could not be removed (the take returned nothing), leaving a permanently
-- stale checklist in the chat showing work that had long since finished.
-- Observed on 2026-07-26: a card still showing the final task unchecked
-- hours after the plan completed and archived correctly (#809).
--
-- One row per session, so re-rendering after a restart targets the same
-- message rather than posting a duplicate. `signature` carries the last
-- rendered content hash, which is what suppresses no-op edits; losing it on
-- restart also lost that dedup.
CREATE TABLE IF NOT EXISTS plan_cards (
    session_id TEXT PRIMARY KEY NOT NULL,
    chat_id    INTEGER NOT NULL,
    thread_id  INTEGER,
    message_id INTEGER NOT NULL,
    signature  TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
