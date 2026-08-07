//! systemd units for the evolve restart path.
//!
//! Split out of `evolve.rs` (#963 review): these are self-contained command
//! builders with no dependency on the tool or its strategies, and pinning
//! their arg lists in tests is the whole point — silent drift in any flag
//! re-introduces the "Evolved! but the daemon never restarted" symptom.

/// Service-unit glob used by the systemd restart path. Matches every
/// profile (default, ops, staging, ...) sharing the same binary.
pub(crate) const SYSTEMD_UNIT_PATTERN: &str = "opencrabs*.service";

/// Build the `systemd-run` command that schedules a delayed restart
/// of every service unit matching `SYSTEMD_UNIT_PATTERN`. Extracted
/// so the arg list can be pinned by tests — silent drift in any of
/// these flags would re-introduce the "Evolved! but daemon didn't
/// restart" symptom that issue #136 reported.
///
/// Set `user` to `true` to target user-level units (`systemctl --user`),
/// e.g. when OpenCrabs was installed via `install_systemd_service()` which
/// writes to `~/.config/systemd/user/`.
///
/// The `pid` argument is used to derive a unique transient unit
/// name (`opencrabs-evolve-<pid>`) so concurrent evolve calls don't
/// collide on the transient unit registry.
pub(crate) fn build_systemd_restart_command(pid: u32, user: bool) -> std::process::Command {
    let unit_name = format!("opencrabs-evolve-{pid}");
    let mut cmd = std::process::Command::new("systemd-run");
    let mut args = vec![];
    // --user on systemd-run itself is required when the daemon runs as a
    // user service: without it, systemd-run tries to talk to the system
    // bus and either fails (no permission from within a --user service)
    // or creates the transient timer in the system instance, where the
    // spawned systemctl won't have DBUS_SESSION_BUS_ADDRESS available.
    if user {
        args.push("--user".to_string());
    }
    args.push("--on-active=3".to_string());
    args.push(format!("--unit={unit_name}"));
    args.push("systemctl".to_string());
    // --user on systemctl is needed to target the user service manager.
    if user {
        args.push("--user".to_string());
    }
    args.push("restart".to_string());
    args.push(SYSTEMD_UNIT_PATTERN.to_string());
    cmd.args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    cmd
}

/// Glob matching every transient evolve restart unit we have ever
/// scheduled. `build_systemd_restart_command` embeds the scheduling
/// process PID in each unit name (`opencrabs-evolve-<pid>`), so over a
/// host's lifetime many such units are created — one per evolve / auto
/// update attempt.
pub(crate) const EVOLVE_UNIT_GLOB: &str = "opencrabs-evolve-*.service";

/// Build the command that garbage-collects spent evolve restart units.
///
/// We cannot pass `--collect` to `systemd-run` (it's unsupported on
/// systemd < v240, RHEL 7 / CentOS 7), so a finished transient
/// `opencrabs-evolve-<pid>` unit lingers in systemd's registry — and a
/// restart that *fails* (e.g. it lost the channel-token race against a
/// running TUI) lingers in the **failed** state, accumulating forever.
/// `systemctl reset-failed <glob>` clears those spent units and is
/// available on every systemd version we target, so we call it right
/// before scheduling a fresh restart. Best-effort: its failure must
/// never block the evolve.
pub(crate) fn build_systemd_cleanup_command(user: bool) -> std::process::Command {
    let mut cmd = std::process::Command::new("systemctl");
    if user {
        cmd.arg("--user");
    }
    cmd.arg("reset-failed")
        .arg(EVOLVE_UNIT_GLOB)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    cmd
}

/// Count systemd service units matching the given glob pattern, at either
/// system or user level.
///
/// Set `user` to `true` to query user-level units (`systemctl --user`).
///
/// Returns `Some(n)` on a successful query (n may be zero), or
/// `None` if `systemctl` failed to spawn / returned a non-zero exit
/// status (a permissions issue or non-systemd host). `None` is a
/// "don't know" signal: the caller should fall through and schedule
/// the restart anyway rather than blocking on a diagnostic failure.
///
/// Uses `--no-legend --no-pager` to keep stdout machine-parseable.
/// Counts non-empty lines — `systemctl` prints one line per matched
/// unit when `--no-legend` is set.
pub(crate) fn count_matching_systemd_units(pattern: &str, user: bool) -> Option<usize> {
    let mut cmd = std::process::Command::new("systemctl");
    cmd.args(["list-units", "--no-legend", "--no-pager"]);
    if user {
        cmd.arg("--user");
    }
    cmd.arg(pattern);
    cmd.stderr(std::process::Stdio::null());
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Some(stdout.lines().filter(|l| !l.trim().is_empty()).count())
}
