#!/usr/bin/env bash
#
# health-check.sh - ping a list of URLs and check disk usage, print a report.
#
# Cron-safe: declares its own PATH and uses absolute paths. Designed for the
# script-runner pattern - the cron prompt is just:
#   "Run ~/.opencrabs/scripts/health-check.sh and reply with its output."
#
# Generic template. Edit the CONFIG block, then `chmod +x` and test by hand.

set -euo pipefail
export PATH="/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"

# ----- CONFIG ---------------------------------------------------------------
# URLs to probe. Replace with your own endpoints.
URLS=(
  "https://example.com"
  "https://api.example.com/health"
)
# Disk mount to watch and the percent that counts as "warn".
DISK_MOUNT="/"
DISK_WARN_PCT=85
# Per-request timeout in seconds.
TIMEOUT=10
# ----------------------------------------------------------------------------

problems=0

echo "Health check - $(date -u '+%Y-%m-%d %H:%M:%S UTC')"
echo

echo "Endpoints:"
for url in "${URLS[@]}"; do
  code="$(curl -s -o /dev/null -w '%{http_code}' --max-time "$TIMEOUT" "$url" || echo "000")"
  if [ "$code" = "200" ]; then
    echo "  OK    $code  $url"
  else
    echo "  FAIL  $code  $url"
    problems=$((problems + 1))
  fi
done

echo
echo "Disk ($DISK_MOUNT):"
used_pct="$(df -P "$DISK_MOUNT" | awk 'NR==2 {gsub(/%/,"",$5); print $5}')"
if [ "$used_pct" -ge "$DISK_WARN_PCT" ]; then
  echo "  WARN  ${used_pct}% used (threshold ${DISK_WARN_PCT}%)"
  problems=$((problems + 1))
else
  echo "  OK    ${used_pct}% used"
fi

echo
if [ "$problems" -eq 0 ]; then
  echo "Status: all green"
else
  echo "Status: $problems problem(s) found"
fi

# Exit non-zero on problems so callers/logs can detect failure.
[ "$problems" -eq 0 ]
