#!/usr/bin/env bash
# Manual Hermes validation checklist for Telegram session fix (issue #121).
# Run from Mac after deploying opencrabs with session_resolve changes.
set -euo pipefail

echo "=== Hermes Telegram session validation ==="
echo "Manual steps in @oc_l1979_bot or ops bot:"
echo "  1. /new → send one message → wait 10s for auto-title"
echo "  2. Send second message → /sessions list must show non-default title"
echo "  3. /sessions → switch older session → send ping → verify context"
echo ""

if ! command -v ssh >/dev/null; then
  echo "ssh not found; skipping DB snapshot"
  exit 0
fi

if ssh -o BatchMode=yes -o ConnectTimeout=10 hermes true 2>/dev/null; then
  echo "=== Recent Telegram sessions (ops profile) ==="
  ssh hermes 'sqlite3 ~/.opencrabs/profiles/ops/opencrabs.db \
    "SELECT substr(title,1,70), auto_title_attempted, datetime(updated_at) \
     FROM sessions WHERE title LIKE \"%Telegram%\" AND archived_at IS NULL \
     ORDER BY updated_at DESC LIMIT 8;"' || true
else
  echo "ssh hermes unavailable — run DB query manually (see README.md)"
fi
