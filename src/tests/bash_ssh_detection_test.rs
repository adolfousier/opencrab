//! Tests for SSH detection in the bash tool.
//!
//! `parse_ssh_invocation` decides whether a bash command is an SSH-like
//! call that may need a password prompt. False positives mean we add a
//! BatchMode probe to commands that don't need it (and would never
//! prompt) — slow but harmless. False negatives mean an SSH command
//! gets spawned without setsid-bypass-aware logic — could still bleed
//! escape sequences into the TUI on older builds without setsid (which
//! does not happen on this branch, but the test guards against
//! regressions in detection).

use crate::brain::tools::bash::parse_ssh_invocation;

#[test]
fn detects_plain_ssh_with_user_at_host() {
    assert_eq!(
        parse_ssh_invocation("ssh root@1.2.3.4 ls"),
        Some("root@1.2.3.4".to_string())
    );
}

#[test]
fn detects_ssh_with_only_hostname() {
    assert_eq!(
        parse_ssh_invocation("ssh iolodev"),
        Some("iolodev".to_string())
    );
}

#[test]
fn detects_ssh_with_identity_flag() {
    assert_eq!(
        parse_ssh_invocation("ssh -i ~/.ssh/mykey root@203.0.113.5"),
        Some("root@203.0.113.5".to_string())
    );
}

#[test]
fn detects_scp_target() {
    assert!(parse_ssh_invocation("scp file.txt user@host:/tmp/").is_some());
}

#[test]
fn detects_rsync_with_ssh_target() {
    assert!(parse_ssh_invocation("rsync -avz local user@host:/tmp/").is_some());
}

#[test]
fn skips_when_batchmode_yes_already_set() {
    // User-supplied BatchMode means they already opted into key-only
    // — don't intercept and don't prompt.
    assert_eq!(parse_ssh_invocation("ssh -o BatchMode=yes root@host"), None);
}

#[test]
fn skips_when_passwordauth_disabled() {
    assert_eq!(
        parse_ssh_invocation("ssh -o PasswordAuthentication=no root@host"),
        None
    );
}

#[test]
fn skips_when_publickey_only() {
    assert_eq!(
        parse_ssh_invocation("ssh -o PreferredAuthentications=publickey root@host"),
        None
    );
}

#[test]
fn ignores_non_ssh_commands() {
    assert!(parse_ssh_invocation("ls -la").is_none());
    assert!(parse_ssh_invocation("git push").is_none());
    assert!(parse_ssh_invocation("docker ps").is_none());
    assert!(parse_ssh_invocation("ssh-keygen -t ed25519").is_none());
}

#[test]
fn detection_is_case_insensitive_for_options() {
    assert_eq!(parse_ssh_invocation("ssh -o BATCHMODE=YES host"), None);
}
