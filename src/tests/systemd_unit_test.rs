//! `opencrabs service install` on Linux: a root (sudo) install must produce a
//! *system* unit that runs as the invoking user and starts on boot, while a
//! non-root install produces a plain *user* unit. (The bus-failure handling and
//! honest exit-status reporting live in `run_systemctl`, which shells out to
//! systemctl and so is validated on the box, not here.)

use crate::cli::commands::build_systemd_unit;

#[test]
fn system_unit_runs_as_invoking_user_and_boots() {
    let unit = build_systemd_unit("default", "/home/me/opencrabs daemon", Some("me"), true);
    assert!(unit.contains("User=me"));
    assert!(unit.contains("Group=me"));
    assert!(unit.contains("Environment=HOME=/home/me"));
    assert!(unit.contains("WantedBy=multi-user.target"));
    assert!(unit.contains("ExecStart=/home/me/opencrabs daemon"));
    // Restart=always (not on-failure) so a clean exit also auto-recovers,
    // matching the macOS LaunchAgent's KeepAlive. The daemon exits 0 on a
    // normal shutdown, which on-failure would leave dead.
    assert!(unit.contains("Restart=always"));
}

#[test]
fn user_unit_has_no_user_directive_and_targets_default() {
    let unit = build_systemd_unit("default", "/home/me/opencrabs daemon", None, false);
    assert!(!unit.contains("User="));
    assert!(!unit.contains("Environment=HOME="));
    assert!(unit.contains("WantedBy=default.target"));
}

#[test]
fn profile_label_is_threaded_into_description() {
    let unit = build_systemd_unit("staging", "/bin/opencrabs -p staging daemon", None, true);
    assert!(unit.contains("Description=OpenCrabs Daemon [staging]"));
}
