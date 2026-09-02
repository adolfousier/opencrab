//! Choosing how to pull a dropped file across from the client (#1289).
//!
//! Dropping a file into a TUI running over SSH inserts the CLIENT's path: the
//! bytes are on the laptop, the process is on the remote. The terminal is the
//! one channel that reaches both, so the tier is decided from what the
//! terminal and the remote can actually do.
//!
//! The decision is a pure function of the environment, which is the whole
//! reason `Env` is passed in rather than read inside it.

use std::collections::HashMap;

use crate::tui::remote_upload::{Channel, Env, choose, guidance, is_remote, multiplexed, scp_hint};

fn env(pairs: &[(&str, &str)]) -> Env {
    Env {
        vars: pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<HashMap<_, _>>(),
        has_kitten: false,
        has_rz: false,
    }
}

const SSH: (&str, &str) = ("SSH_CONNECTION", "192.168.1.42 51234 10.0.0.7 22");

#[test]
fn test_remote_is_detected_from_any_of_the_ssh_markers() {
    assert!(is_remote(&env(&[SSH])));
    assert!(is_remote(&env(&[("SSH_TTY", "/dev/pts/0")])));
    assert!(is_remote(&env(&[("SSH_CLIENT", "192.168.1.42 51234 22")])));
    assert!(!is_remote(&env(&[("TERM", "xterm-256color")])));
    // An empty var is not a marker — set-but-blank is how some shells leave it.
    assert!(!is_remote(&env(&[("SSH_CONNECTION", "")])));
}

#[test]
fn test_kitty_wins_when_both_ends_can_do_it() {
    let mut e = env(&[SSH, ("TERM", "xterm-kitty")]);
    e.has_kitten = true;
    assert_eq!(choose(&e), Channel::Kitty);

    // Terminal is kitty but the remote has no kitten: nothing to run here.
    let e = env(&[SSH, ("TERM", "xterm-kitty")]);
    assert_eq!(choose(&e), Channel::ScpHint);
}

#[test]
fn test_zmodem_for_terminals_that_answer_it() {
    for term in ["iTerm.app", "WezTerm", "tabby"] {
        let mut e = env(&[SSH, ("TERM_PROGRAM", term)]);
        e.has_rz = true;
        assert_eq!(choose(&e), Channel::Zmodem, "{term} should use zmodem");
    }
    // rz present but the terminal is not known to answer it: emitting the
    // sequence into a terminal that ignores it prints garbage, so don't.
    let mut e = env(&[SSH, ("TERM_PROGRAM", "Apple_Terminal")]);
    e.has_rz = true;
    assert_eq!(choose(&e), Channel::ScpHint);
}

#[test]
fn test_a_multiplexer_forces_the_hint_tier() {
    // tmux and screen rewrite the escape stream, so an in-band transfer is
    // swallowed. Better to say so than to emit something that fails silently.
    let mut e = env(&[
        SSH,
        ("TERM", "xterm-kitty"),
        ("TMUX", "/tmp/tmux-0/default"),
    ]);
    e.has_kitten = true;
    assert_eq!(choose(&e), Channel::ScpHint);

    let mut e = env(&[SSH, ("TERM_PROGRAM", "iTerm.app"), ("STY", "1234.pts-0")]);
    e.has_rz = true;
    assert_eq!(choose(&e), Channel::ScpHint);

    assert!(multiplexed(&env(&[("TMUX", "x")])));
    assert!(!multiplexed(&env(&[SSH])));
}

#[test]
fn test_the_scp_hint_names_the_client_host_and_survives_spaces() {
    let e = env(&[SSH]);
    let hint = scp_hint(
        &e,
        "/Users/me/Screenshot 2026-09-01 at 18.18.16.png",
        "/root/att",
    );
    assert!(
        hint.contains("192.168.1.42"),
        "the client is the first field of SSH_CONNECTION: {hint}"
    );
    assert!(
        hint.contains("'/Users/me/Screenshot 2026-09-01 at 18.18.16.png'"),
        "#1289: a spaced path must be quoted or the hint does not run: {hint}"
    );
    assert!(hint.ends_with("/root/att/"));
}

#[test]
fn test_a_quote_in_the_path_cannot_break_out_of_the_hint() {
    let e = env(&[SSH]);
    let hint = scp_hint(&e, "/tmp/it's here.png", "/dest");
    assert!(
        hint.contains(r"'/tmp/it'\''s here.png'"),
        "embedded quote must be escaped: {hint}"
    );
}

#[test]
fn test_scp_is_always_the_command_given() {
    // It is the one mechanism that works from every terminal, through tmux,
    // with nothing installed. A better tier is mentioned, never substituted.
    let mut kitty = env(&[SSH, ("TERM", "xterm-kitty")]);
    kitty.has_kitten = true;
    let g = guidance(&kitty, "/Users/me/a b.png", "/root/att");
    assert!(g.contains("scp"), "{g}");
    assert!(
        g.contains("'/Users/me/a b.png'"),
        "spaces must survive: {g}"
    );
    assert!(
        g.contains("kitten"),
        "the better tier is still surfaced: {g}"
    );

    let mut iterm = env(&[SSH, ("TERM_PROGRAM", "iTerm.app")]);
    iterm.has_rz = true;
    let g = guidance(&iterm, "/Users/me/a.png", "/root/att");
    assert!(g.contains("scp"), "{g}");
    assert!(g.contains("rz"), "{g}");

    let g = guidance(&env(&[SSH]), "/Users/me/a.png", "/root/att");
    assert!(g.contains("scp"), "{g}");
}

#[test]
fn test_no_unverified_command_line_is_presented_as_runnable() {
    // kitty's and zmodem's invocations are not exercised from this repo, so
    // they are named as options, never handed over as a command to paste.
    let mut kitty = env(&[SSH, ("TERM", "xterm-kitty")]);
    kitty.has_kitten = true;
    let g = guidance(&kitty, "/Users/me/a.png", "/root/att");
    assert!(
        g.contains("--help"),
        "point at the terminal's own docs rather than asserting flags: {g}"
    );
}

#[test]
fn test_a_local_session_is_told_the_path_is_simply_wrong() {
    // Not SSH: a missing path is a typo, not a transfer problem, and
    // suggesting scp would be noise.
    let g = guidance(&env(&[("TERM", "xterm-256color")]), "/nope/a.png", "/dest");
    assert!(g.contains("does not exist here"), "{g}");
    assert!(
        !g.contains("scp"),
        "no transfer advice when not remote: {g}"
    );
}

#[test]
fn test_the_multiplexer_case_explains_itself() {
    let g = guidance(&env(&[SSH, ("TMUX", "x")]), "/Users/me/a.png", "/dest");
    assert!(g.contains("scp"), "{g}");
    assert!(
        g.contains("tmux/screen"),
        "the user should learn WHY the better tier is unavailable: {g}"
    );
}
