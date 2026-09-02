//! Getting a dropped file from the terminal's machine onto this one (#1289).
//!
//! OpenCrabs lives in a terminal, and a terminal is routinely a window onto
//! another machine. Dropping a file into a session running over SSH inserts
//! the CLIENT's path: the bytes are on the laptop, the process is on the
//! remote, and nothing local can reach them.
//!
//! The terminal itself is the channel. Two protocols already move a file from
//! the client to a program running on the far end, and both are driven from
//! this side:
//!
//! * kitty's `kitten transfer`, over kitty's own escape protocol
//! * zmodem `rz`, which iTerm2 and others answer with a file picker
//!
//! Neither is universal, so the floor matters as much as the ceiling: when no
//! protocol is available the user gets a runnable `scp` line rather than a
//! shrug.
//!
//! Detection is environment reading and is unit-tested. The transfer itself
//! is not testable without a terminal, so it sits behind [`Channel`]: the tier
//! decision is a pure function of the environment, and only the execution of
//! a chosen tier touches the world.

use std::collections::HashMap;

/// How a dropped file can be pulled across, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Channel {
    /// kitty's own transfer protocol. Needs `kitten` on this side.
    Kitty,
    /// zmodem receive. The terminal answers with its own file picker.
    Zmodem,
    /// No in-band transfer. Print an `scp` line the user can paste.
    ScpHint,
}

/// The environment inputs the tier decision reads.
///
/// Taken as a struct rather than read from `std::env` inside the decision so
/// the choice is a pure function and every branch is testable.
#[derive(Debug, Default, Clone)]
pub(crate) struct Env {
    pub vars: HashMap<String, String>,
    /// Whether a command is on `PATH` here. Injected for the same reason.
    pub has_kitten: bool,
    pub has_rz: bool,
}

impl Env {
    /// Read the live environment. Command lookups are done once, here.
    pub(crate) fn current() -> Self {
        Self {
            vars: std::env::vars().collect(),
            has_kitten: on_path("kitten"),
            has_rz: on_path("rz"),
        }
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.vars
            .get(key)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }
}

/// Is this process being driven from another machine?
///
/// `SSH_CONNECTION` / `SSH_TTY` are what every shell prompt uses for the same
/// question. Without one of them a missing path is simply a wrong path, and
/// suggesting a transfer would be noise.
pub(crate) fn is_remote(env: &Env) -> bool {
    env.has("SSH_CONNECTION") || env.has("SSH_TTY") || env.has("SSH_CLIENT")
}

/// Is a terminal multiplexer between us and the terminal emulator?
///
/// tmux and screen rewrite the escape stream. A transfer sequence emitted
/// through one is eaten or mangled rather than answered, so the honest move
/// is to drop to the hint tier instead of emitting something that will fail
/// silently.
pub(crate) fn multiplexed(env: &Env) -> bool {
    env.has("TMUX") || env.has("STY")
}

/// Pick the transfer channel for this environment.
///
/// Only ever called once a drop is known to be unreachable locally.
pub(crate) fn choose(env: &Env) -> Channel {
    if multiplexed(env) {
        return Channel::ScpHint;
    }
    if env.has_kitten && is_kitty(env) {
        return Channel::Kitty;
    }
    if env.has_rz && zmodem_capable(env) {
        return Channel::Zmodem;
    }
    Channel::ScpHint
}

/// Running inside kitty. `TERM` is set by kitty's terminfo; the window id is
/// set even when `TERM` has been overridden.
fn is_kitty(env: &Env) -> bool {
    env.get("TERM").is_some_and(|t| t.contains("kitty")) || env.has("KITTY_WINDOW_ID")
}

/// Terminals that answer a zmodem receive with their own file picker.
///
/// Deliberately an allowlist. Emitting the sequence into a terminal that does
/// not understand it prints garbage into the user's session, so an unknown
/// terminal falls through to the hint rather than being probed.
fn zmodem_capable(env: &Env) -> bool {
    matches!(
        env.get("TERM_PROGRAM"),
        Some("iTerm.app") | Some("WezTerm") | Some("tabby")
    ) || env.has("ITERM_SESSION_ID")
}

/// The `scp` line that moves the file here, to be run ON THE CLIENT.
///
/// Direction matters and the obvious one is wrong. Pulling from here means
/// this box opening an SSH connection back into the user's machine, which
/// needs an sshd running there, a route through their NAT, and a key of ours
/// they have authorised. On a laptop all three are normally false, and the
/// last is a security downgrade nobody wants in order to attach a screenshot.
///
/// Pushing works, because the client already proved it can reach us: that is
/// how the session exists at all. So the command is theirs to run, against
/// the very address they connected to.
///
/// `SSH_CONNECTION` is `client_ip client_port server_ip server_port`, so the
/// THIRD field is this host as the client addressed it. The login name is
/// left as a placeholder because ours is not necessarily theirs.
pub(crate) fn scp_hint(env: &Env, client_path: &str, dest_dir: &str) -> String {
    let here = env
        .get("SSH_CONNECTION")
        .and_then(|c| c.split_whitespace().nth(2))
        .unwrap_or("<this-host>");
    let user = env.get("USER").unwrap_or("<you>");
    format!(
        "scp {} {user}@{here}:{}/",
        shell_quote(client_path),
        dest_dir
    )
}

/// Single-quote a path for a shell, escaping any embedded quote.
///
/// Dropped paths routinely contain spaces, which is the whole reason #1288
/// existed; a hint that breaks on them would be worse than none.
fn shell_quote(path: &str) -> String {
    format!("'{}'", path.replace('\'', r"'\''"))
}

fn on_path(cmd: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(cmd).is_file())
}

/// What to tell the user about a file that is on their machine, not this one.
///
/// `scp` is always the answer given, because it is the one mechanism that
/// works from every terminal, through `tmux`, and with no extra install. A
/// terminal that can do better gets a note saying so, deliberately WITHOUT a
/// command line: kitty's and zmodem's invocations are not exercised from this
/// repo, and handing someone a flag string that fails is worse than pointing
/// them at their terminal's own documentation.
pub(crate) fn guidance(env: &Env, client_path: &str, dest_dir: &str) -> String {
    if !is_remote(env) {
        // Not an SSH session, so the path is simply wrong rather than remote.
        return format!("{client_path} does not exist here.");
    }

    let mut out = format!(
        "{client_path} is on your machine, not this one. Run this ON YOUR MACHINE:\n  {}",
        scp_hint(env, client_path, dest_dir)
    );

    match choose(env) {
        Channel::Kitty => {
            out.push_str("\n(kitty can also transfer it in-band: see `kitten transfer --help`)")
        }
        Channel::Zmodem => {
            out.push_str("\n(your terminal also answers zmodem: run `rz` here for a file picker)")
        }
        Channel::ScpHint if multiplexed(env) => out.push_str(
            "\n(tmux/screen rewrites the escape stream, so in-band transfer is unavailable)",
        ),
        Channel::ScpHint => {}
    }
    out
}
