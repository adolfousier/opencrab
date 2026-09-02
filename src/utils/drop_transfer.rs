//! Pulling a dropped file across the SSH connection the user already made
//! (#1289).
//!
//! When the TUI runs on a VPS, a dragged file inserts the CLIENT's path. The
//! bytes are on the laptop and the process is on the server, and a process on
//! the server has no handle on the SSH connection it arrived over: sshd gives
//! it a pty and nothing else. So "just use the same connection" is not
//! something this side can do unaided.
//!
//! It becomes possible the moment the user's `ssh` opens a reverse forward:
//!
//! ```text
//! ssh -R 8765:localhost:8765 root@vps
//! ```
//!
//! That is a real channel on the SAME connection, tunnelling back to a small
//! agent on the client. No sshd on the client, no NAT traversal, no trust
//! beyond the shell they already granted, and it works from any terminal and
//! through tmux, because it never touches the pty.
//!
//! This module is the wire format and the guard around it. The guard is the
//! important half: the agent serves the client's filesystem to whatever is on
//! the far end of that tunnel, so it must refuse anything outside the roots a
//! user would actually drop from.

use std::path::{Component, Path, PathBuf};

/// Largest file the agent will serve.
///
/// A drop is a screenshot or a document, not a disk image, and an unbounded
/// read over a tunnel is a way to hang the session rather than a feature.
pub const MAX_TRANSFER_BYTES: u64 = 64 * 1024 * 1024;

/// Default port for the reverse forward. Arbitrary, above the privileged
/// range, and documented so both ends agree without negotiation.
pub const DEFAULT_DROP_PORT: u16 = 8765;

/// Env var the VPS side reads to learn the tunnel is available.
pub const DROP_PORT_VAR: &str = "OPENCRABS_DROP_PORT";

/// What the agent answers with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    /// Serving `len` bytes, which follow immediately.
    Ok { len: u64 },
    /// Refused, with a reason the requesting side can show the user.
    Err { reason: String },
}

impl Response {
    /// One line, newline-terminated, ASCII. The body follows an `Ok`.
    pub fn encode(&self) -> String {
        match self {
            Response::Ok { len } => format!("OK {len}\n"),
            Response::Err { reason } => {
                // A newline in the reason would desynchronise the stream.
                format!("ERR {}\n", reason.replace(['\n', '\r'], " "))
            }
        }
    }

    pub fn parse(line: &str) -> Option<Self> {
        let line = line.trim_end_matches(['\n', '\r']);
        if let Some(rest) = line.strip_prefix("OK ") {
            return rest
                .trim()
                .parse::<u64>()
                .ok()
                .map(|len| Response::Ok { len });
        }
        line.strip_prefix("ERR ").map(|reason| Response::Err {
            reason: reason.trim().to_string(),
        })
    }
}

/// Why a path was refused. Kept as a type so the agent logs and the user's
/// message say the same thing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    NotAbsolute,
    Traversal,
    OutsideRoots,
    NotAFile,
    TooLarge { bytes: u64 },
}

impl Refusal {
    pub fn reason(&self) -> String {
        match self {
            Refusal::NotAbsolute => "path is not absolute".into(),
            Refusal::Traversal => "path contains .. traversal".into(),
            Refusal::OutsideRoots => "path is outside the directories this agent serves".into(),
            Refusal::NotAFile => "not a regular file".into(),
            Refusal::TooLarge { bytes } => {
                format!("file is {bytes} bytes, over the {MAX_TRANSFER_BYTES} byte limit")
            }
        }
    }
}

/// Directories the agent will serve from, by default.
///
/// Deliberately the places a person drags files out of. The agent hands the
/// client's filesystem to whatever holds the other end of the tunnel, so the
/// default must not be the whole home directory: a compromised server asking
/// for `~/.ssh/id_ed25519` has to be refused by construction rather than by
/// the operator remembering to restrict it.
pub fn default_roots() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    ["Desktop", "Downloads", "Pictures", "Documents", "Movies"]
        .iter()
        .map(|d| home.join(d))
        .collect()
}

/// Is `requested` a path this agent may read?
///
/// Checked lexically BEFORE touching the filesystem, so a traversal is
/// refused without a stat, and then confirmed against the resolved path so a
/// symlink cannot escape a root it appears to sit inside.
pub fn authorize(requested: &str, roots: &[PathBuf]) -> Result<PathBuf, Refusal> {
    let path = Path::new(requested);
    if !path.is_absolute() {
        return Err(Refusal::NotAbsolute);
    }
    if path.components().any(|c| c == Component::ParentDir) {
        return Err(Refusal::Traversal);
    }

    // Resolve symlinks before the containment test: a link inside Downloads
    // pointing at ~/.ssh must not inherit Downloads' permission.
    let resolved = path.canonicalize().map_err(|_| Refusal::NotAFile)?;
    let meta = std::fs::metadata(&resolved).map_err(|_| Refusal::NotAFile)?;
    if !meta.is_file() {
        return Err(Refusal::NotAFile);
    }
    if meta.len() > MAX_TRANSFER_BYTES {
        return Err(Refusal::TooLarge { bytes: meta.len() });
    }

    let inside = roots.iter().any(|root| {
        root.canonicalize()
            .map(|r| resolved.starts_with(r))
            .unwrap_or(false)
    });
    if !inside {
        return Err(Refusal::OutsideRoots);
    }
    Ok(resolved)
}

/// Environment markers every shell prompt uses to tell an SSH session apart.
/// A non-empty value for any of them means the terminal is on another
/// machine, which is the only situation a drop tunnel can exist in.
pub const SSH_MARKERS: [&str; 3] = ["SSH_CONNECTION", "SSH_TTY", "SSH_CLIENT"];

/// A tunnel the VPS side may dial for a dropped file (#1311).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tunnel {
    pub port: u16,
    /// `true` when [`DROP_PORT_VAR`] named the port. A declared tunnel that
    /// does not answer is an error worth showing. A probed one that does not
    /// answer is the ordinary "no agent running" case and falls back to the
    /// copy guidance without a word.
    pub declared: bool,
}

/// Decide whether there is a tunnel to try, from the environment `var`
/// reads. Pure so every branch is testable.
///
/// [`DROP_PORT_VAR`] set to a port declares the tunnel. Absent (or
/// unparseable) over an SSH session means probe [`DEFAULT_DROP_PORT`], which
/// is what the documented `ssh -R 8765:localhost:8765` opens: nobody should
/// have to export a variable on the server for the documented flow to work.
/// Off SSH there is nothing to dial.
pub fn tunnel_from(var: impl Fn(&str) -> Option<String>) -> Option<Tunnel> {
    if let Some(port) = var(DROP_PORT_VAR).and_then(|v| v.trim().parse().ok()) {
        return Some(Tunnel {
            port,
            declared: true,
        });
    }
    let over_ssh = SSH_MARKERS
        .iter()
        .any(|k| var(k).is_some_and(|v| !v.trim().is_empty()));
    over_ssh.then_some(Tunnel {
        port: DEFAULT_DROP_PORT,
        declared: false,
    })
}

/// [`tunnel_from`] over the live process environment.
pub fn tunnel() -> Option<Tunnel> {
    tunnel_from(|k| std::env::var(k).ok())
}

/// The `ssh` invocation that opens the tunnel, for documentation and for the
/// message shown when no tunnel is present.
pub fn ssh_hint(user_and_host: &str, port: u16) -> String {
    format!("ssh -R {port}:localhost:{port} {user_and_host}")
}
