#!/usr/bin/env bash
#
# digest.sh - aggregate lines from a source and print a compact daily digest.
#
# Cron-safe: declares its own PATH and uses absolute paths. Designed for the
# script-runner pattern - the cron prompt is just:
#   "Run ~/.opencrabs/scripts/digest.sh and reply with its output."
#
# Example use: summarize today's lines from a log file, a feed dumped to disk,
# or any newline-delimited source. Generic template - edit the CONFIG block,
# then `chmod +x` and test by hand.

set -euo pipefail
export PATH="/usr/local/bin:/usr/bin:/bin"

# ----- CONFIG ---------------------------------------------------------------
# File to digest. Use an absolute path.
SOURCE_FILE="$HOME/data/events.log"
# Only count lines matching this pattern (use ".*" to count everything).
MATCH=".*"
# How many of the most recent matching lines to show.
SHOW_LAST=10
# ----------------------------------------------------------------------------

echo "Daily digest - $(date -u '+%Y-%m-%d %H:%M:%S UTC')"
echo

if [ ! -f "$SOURCE_FILE" ]; then
  echo "ERROR: source file not found: $SOURCE_FILE"
  exit 1
fi

total="$(grep -cE "$MATCH" "$SOURCE_FILE" || true)"
echo "Matching lines: $total"

if [ "$total" -gt 0 ]; then
  echo
  echo "Most recent $SHOW_LAST:"
  grep -E "$MATCH" "$SOURCE_FILE" | tail -n "$SHOW_LAST" | sed 's/^/  /'
fi

echo
echo "Status: digest ok"
