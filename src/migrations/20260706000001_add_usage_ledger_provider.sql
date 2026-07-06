-- Usage ledger provider column (#402): breakdowns by provider need the
-- provider recorded per entry. The ledger outlives sessions, so relying on
-- a join loses provider info once a session is deleted.
ALTER TABLE usage_ledger ADD COLUMN provider TEXT NOT NULL DEFAULT '';

-- Backfill from sessions that still exist (best effort; deleted sessions'
-- historical rows stay '', rendered as "unknown").
UPDATE usage_ledger
SET provider = COALESCE(
    (SELECT s.provider_name FROM sessions s WHERE s.id = usage_ledger.session_id),
    ''
)
WHERE provider = '';
