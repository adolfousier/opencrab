//! Outcome of the post-swap restart-scheduling step, and the user-facing
//! success line derived from it. Never says "Restarting" when no restart
//! was actually scheduled (the original #136 symptom).

use super::systemd::SYSTEMD_UNIT_PATTERN;

/// Outcome of the post-swap restart-scheduling step. Used to tailor
/// the user-facing success string so we never say "Restarting…" when
/// no restart was actually scheduled (the original #136 symptom — we
/// must not reintroduce it for a new reason).
#[derive(Debug)]
pub(super) enum RestartStatus {
    /// Not running on a systemd host (no `/run/systemd/system`). The
    /// caller's `RestartReady` progress event is the only restart
    /// signal — e.g. cargo-install / TUI launch paths handle that.
    NotSystemd,
    /// `systemctl list-units` matched zero units. systemd is present
    /// but nothing in the unit registry corresponds to opencrabs;
    /// scheduling a restart would be a no-op so we don't.
    NoUnitsMatched,
    /// systemd-run was spawned successfully — restart fires in 3s.
    Scheduled,
    /// systemd-run failed to spawn (binary missing on this host,
    /// permission denied, etc.). Carries the error string so the
    /// user-visible message can quote it for forensics.
    SpawnFailed(String),
}

impl RestartStatus {
    pub(super) fn user_message(&self, current: &str, latest: &str) -> String {
        match self {
            RestartStatus::Scheduled => {
                format!("Evolved from v{current} to v{latest}.")
            }
            RestartStatus::NotSystemd => format!(
                "Evolved from v{current} to v{latest}. Binary updated on disk; restart \
                 the process / relaunch to load the new version."
            ),
            RestartStatus::NoUnitsMatched => format!(
                "Evolved from v{current} to v{latest}. Binary updated on disk, but no \
                 systemd units matched `{SYSTEMD_UNIT_PATTERN}` at system or user level \
                 — your daemon (if any) was not restarted. Restart it manually with \
                 `systemctl --user restart {SYSTEMD_UNIT_PATTERN}` (if installed as a \
                 user service) or `systemctl restart <your-unit>` (if a system service), \
                 or relaunch if running standalone."
            ),
            RestartStatus::SpawnFailed(err) => format!(
                "Evolved from v{current} to v{latest}. Binary updated on disk, but \
                 scheduling the systemd restart failed ({err}). Restart your daemon \
                 manually with `systemctl --user restart {SYSTEMD_UNIT_PATTERN}` \
                 (if a user service) or `systemctl restart {SYSTEMD_UNIT_PATTERN}` \
                 (if a system service)."
            ),
        }
    }
}
