#!/usr/bin/env bash
set -euo pipefail

# OpenCrabs Watchdog — monitors the opencrabs chat pane in tmux
# Runs via opencrabs-watchdog.timer (systemd) every 5 minutes.
#
# Behavior:
#   A) Kills ONLY the pane running opencrabs chat, not the entire session
#   B) Warns the user 30s before killing (tmux display-message)
#   C) Auto-reattaches the user to the new pane after restart
#
# Usage:
#   1. Copy to ~/.opencrabs/scripts/watchdog.sh
#   2. Install systemd units (see below)
#   3. Start the timer: systemctl --user enable --now opencrabs-watchdog.timer
#
# Install systemd units:
#   sudo cp opencrabs.service /usr/local/lib/systemd/system/opencrabs-daemon.service
#   cp opencrabs-watchdog.timer ~/.config/systemd/user/opencrabs-watchdog.timer
#   cp opencrabs-watchdog.service ~/.config/systemd/user/opencrabs-watchdog.service
#   systemctl --user daemon-reload
#   systemctl --user enable --now opencrabs-watchdog.timer

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

TMUX_SESSION="${OPENCRABS_TMUX_SESSION:-opencrabs}"
TMUX_PANE="${OPENCRABS_TMUX_PANE:-0.0}"
OPENCRABS_BIN="${OPENCRABS_BIN:-opencrabs}"
OPENCRABS_ARGS="${OPENCRABS_ARGS:-chat --debug}"
WARN_SECONDS="${OPENCRABS_WARN_SECONDS:-30}"
LOG_FILE="${OPENCRABS_WATCHDOG_LOG:-/tmp/opencrabs-watchdog.log}"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

log() {
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*" >> "$LOG_FILE"
}

# Check if the tmux session exists
session_exists() {
    tmux has-session -t "$TMUX_SESSION" 2>/dev/null
}

# Check if the opencrabs process is running in the target pane
process_running() {
    local pane_pid
    pane_pid=$(tmux display-message -t "${TMUX_SESSION}:${TMUX_PANE}" -p '#{pane_pid}' 2>/dev/null) || return 1

    # Walk the process tree from the pane's shell PID
    local child_pids
    child_pids=$(pgrep -P "$pane_pid" 2>/dev/null) || return 1

    for pid in $child_pids; do
        if ps -p "$pid" -o args= 2>/dev/null | grep -q "opencrabs.*chat"; then
            return 0
        fi
    done
    return 1
}

# Count panes in the tmux session (excludes the current one if >1)
pane_count() {
    tmux list-panes -t "$TMUX_SESSION" 2>/dev/null | wc -l | tr -d ' '
}

# Send a warning message to the tmux status bar
warn_user() {
    tmux display-message -t "${TMUX_SESSION}:${TMUX_PANE}" \
        "Watchdog: opencrabs chat not responding. Restarting in ${WARN_SECONDS}s..." 2>/dev/null || true
    log "WARNING: Sent ${WARN_SECONDS}s kill warning to pane ${TMUX_PANE}"
}

# Kill only the target pane (A: preserve session and other panes)
kill_pane() {
    local count
    count=$(pane_count)

    if [ "$count" -gt 1 ]; then
        # Other panes exist — kill only the target pane
        tmux kill-pane -t "${TMUX_SESSION}:${TMUX_PANE}" 2>/dev/null || true
        log "KILLED pane ${TMUX_PANE} (session preserved, ${count} panes were open)"
    else
        # Only one pane — killing it kills the session, so just kill the pane
        tmux kill-pane -t "${TMUX_SESSION}:${TMUX_PANE}" 2>/dev/null || true
        log "KILLED pane ${TMUX_PANE} (was the last pane, session will close)"
    fi
}

# Restart opencrabs in a new pane (C: auto-reattach)
restart_opencrabs() {
    if session_exists; then
        # Session still alive — create a new window
        tmux new-window -t "$TMUX_SESSION" -n opencrabs "${OPENCRABS_BIN} ${OPENCRABS_ARGS}" 2>/dev/null || true
        log "RESTARTED: new window in existing session ${TMUX_SESSION}"
    else
        # Session gone — recreate it
        tmux new-session -d -s "$TMUX_SESSION" "${OPENCRABS_BIN} ${OPENCRABS_ARGS}" 2>/dev/null || true
        log "RESTARTED: new session ${TMUX_SESSION}"
    fi
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

main() {
    # Session doesn't exist — nothing to monitor
    if ! session_exists; then
        log "Session ${TMUX_SESSION} not found. Starting fresh."
        restart_opencrabs
        exit 0
    fi

    # Process is healthy — nothing to do
    if process_running; then
        exit 0
    fi

    # Process is dead — warn, wait, kill pane, restart
    log "DETECTED: opencrabs chat not running in ${TMUX_SESSION}:${TMUX_PANE}"

    # B: Warn the user
    warn_user

    # B: Give the user time to intervene
    sleep "$WARN_SECONDS"

    # Re-check: maybe the user restarted it manually during the warning
    if process_running; then
        log "User restarted opencrabs during warning window. Aborting."
        exit 0
    fi

    # A: Kill only the pane, not the session
    kill_pane

    # C: Restart and auto-reattach
    restart_opencrabs

    log "WATCHDOG: restart complete for ${TMUX_SESSION}"
}

main "$@"
