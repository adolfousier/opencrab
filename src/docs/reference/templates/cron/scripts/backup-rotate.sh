#!/usr/bin/env bash
#
# backup-rotate.sh - tar a source dir into a dated archive, prune old ones.
#
# Cron-safe: declares its own PATH and uses absolute paths. Designed for the
# script-runner pattern - the cron prompt is just:
#   "Run ~/.opencrabs/scripts/backup-rotate.sh and reply with its output."
#
# Generic template. Edit the CONFIG block, then `chmod +x` and test by hand.

set -euo pipefail
export PATH="/usr/local/bin:/usr/bin:/bin"

# ----- CONFIG ---------------------------------------------------------------
# Directory to back up. Use an absolute path.
SOURCE_DIR="$HOME/data"
# Where archives are written. Created if missing.
BACKUP_DIR="$HOME/backups"
# Delete archives older than this many days.
RETENTION_DAYS=14
# ----------------------------------------------------------------------------

echo "Backup - $(date -u '+%Y-%m-%d %H:%M:%S UTC')"
echo

if [ ! -d "$SOURCE_DIR" ]; then
  echo "ERROR: source dir not found: $SOURCE_DIR"
  exit 1
fi

mkdir -p "$BACKUP_DIR"

stamp="$(date -u '+%Y%m%d-%H%M%S')"
base="$(basename "$SOURCE_DIR")"
archive="$BACKUP_DIR/${base}-${stamp}.tar.gz"

echo "Creating $archive"
tar -czf "$archive" -C "$(dirname "$SOURCE_DIR")" "$base"
size="$(du -h "$archive" | awk '{print $1}')"
echo "  done ($size)"

echo
echo "Pruning archives older than ${RETENTION_DAYS} days:"
pruned=0
while IFS= read -r old; do
  rm -f "$old"
  echo "  removed $(basename "$old")"
  pruned=$((pruned + 1))
done < <(find "$BACKUP_DIR" -maxdepth 1 -name "${base}-*.tar.gz" -type f -mtime "+${RETENTION_DAYS}")
[ "$pruned" -eq 0 ] && echo "  nothing to prune"

echo
remaining="$(find "$BACKUP_DIR" -maxdepth 1 -name "${base}-*.tar.gz" -type f | wc -l | tr -d ' ')"
echo "Status: backup ok, $remaining archive(s) retained"
