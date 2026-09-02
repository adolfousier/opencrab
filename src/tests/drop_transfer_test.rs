//! The drop tunnel: wire format, and the guard around the client's files
//! (#1289).
//!
//! A process on the VPS has no handle on the SSH connection it arrived over,
//! so "use the same connection" only becomes possible once the user opens a
//! reverse forward. The agent then serves their filesystem to whatever holds
//! the other end of that tunnel, which is why the authorisation tests below
//! matter more than the framing ones.

use std::path::PathBuf;

use crate::utils::drop_agent;
use crate::utils::drop_transfer::{
    DEFAULT_DROP_PORT, DROP_PORT_VAR, MAX_TRANSFER_BYTES, Refusal, Response, Tunnel, authorize,
    ssh_hint, tunnel_from,
};

struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!("oc-tunnel-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("served")).expect("sandbox");
        std::fs::create_dir_all(root.join("secret")).expect("sandbox");
        std::fs::write(root.join("served/shot 1.png"), b"\x89PNG\r\n\x1a\nbody").expect("file");
        std::fs::write(root.join("secret/id_ed25519"), b"PRIVATE KEY").expect("file");
        Self { root }
    }
    fn served(&self) -> PathBuf {
        self.root.join("served")
    }
    fn path(&self, rel: &str) -> String {
        self.root.join(rel).to_string_lossy().to_string()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

// ── The guard ─────────────────────────────────────────────────────────────

#[test]
fn test_a_file_inside_a_served_root_is_allowed() {
    let s = Sandbox::new("allow");
    let ok = authorize(&s.path("served/shot 1.png"), &[s.served()]);
    assert!(ok.is_ok(), "{ok:?}");
}

#[test]
fn test_a_file_outside_the_roots_is_refused() {
    // The whole reason the default is Desktop/Downloads and not $HOME: a
    // compromised server asking for a private key must be refused by
    // construction, not by the operator having remembered to restrict it.
    let s = Sandbox::new("outside");
    assert_eq!(
        authorize(&s.path("secret/id_ed25519"), &[s.served()]),
        Err(Refusal::OutsideRoots)
    );
}

#[test]
fn test_traversal_is_refused_before_touching_the_filesystem() {
    let s = Sandbox::new("traverse");
    let sneaky = format!("{}/served/../secret/id_ed25519", s.root.to_string_lossy());
    assert_eq!(authorize(&sneaky, &[s.served()]), Err(Refusal::Traversal));
}

#[cfg(unix)]
#[test]
fn test_a_symlink_cannot_escape_a_served_root() {
    // A link INSIDE Downloads pointing at ~/.ssh must not inherit Downloads'
    // permission, which is why containment is tested on the resolved path.
    let s = Sandbox::new("symlink");
    let link = s.root.join("served/innocent.png");
    std::os::unix::fs::symlink(s.root.join("secret/id_ed25519"), &link).expect("symlink");
    assert_eq!(
        authorize(&link.to_string_lossy(), &[s.served()]),
        Err(Refusal::OutsideRoots)
    );
}

#[test]
fn test_relative_paths_and_directories_are_refused() {
    let s = Sandbox::new("shape");
    assert_eq!(
        authorize("served/shot 1.png", &[s.served()]),
        Err(Refusal::NotAbsolute)
    );
    assert_eq!(
        authorize(&s.path("served"), &[s.served()]),
        Err(Refusal::NotAFile)
    );
    assert_eq!(
        authorize(&s.path("served/missing.png"), &[s.served()]),
        Err(Refusal::NotAFile)
    );
}

// ── Wire format ───────────────────────────────────────────────────────────

#[test]
fn test_response_round_trips() {
    assert_eq!(
        Response::parse(&Response::Ok { len: 42 }.encode()),
        Some(Response::Ok { len: 42 })
    );
    let err = Response::Err {
        reason: "nope".into(),
    };
    assert_eq!(Response::parse(&err.encode()), Some(err));
    assert_eq!(Response::parse("garbage\n"), None);
}

#[test]
fn test_a_newline_in_a_reason_cannot_desync_the_stream() {
    let encoded = Response::Err {
        reason: "line one\nline two".into(),
    }
    .encode();
    assert_eq!(encoded.matches('\n').count(), 1, "{encoded:?}");
}

#[test]
fn test_the_ssh_hint_forwards_the_port_both_ways() {
    let hint = ssh_hint("root@example.com", DEFAULT_DROP_PORT);
    assert_eq!(hint, "ssh -R 8765:localhost:8765 root@example.com");
}

// ── End to end over a real socket ─────────────────────────────────────────

#[test]
fn test_fetch_pulls_the_bytes_and_refusals_surface_as_errors() {
    let s = Sandbox::new("e2e");
    let roots = vec![s.served()];

    // Port 0 lets the OS pick, so the test cannot collide with a real agent.
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind an ephemeral port");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);

    let serve_roots = roots.clone();
    std::thread::spawn(move || {
        let _ = drop_agent::serve(port, serve_roots);
    });
    // The agent binds on its own thread; retry briefly rather than sleeping a
    // fixed amount and hoping.
    let mut got = None;
    for _ in 0..50 {
        match drop_agent::fetch(port, &s.path("served/shot 1.png")) {
            Ok(bytes) => {
                got = Some(bytes);
                break;
            }
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(20)),
        }
    }
    let bytes = got.expect("agent never answered");
    assert_eq!(bytes, b"\x89PNG\r\n\x1a\nbody", "wrong bytes came back");

    // A refused path must be an error, never empty bytes that would attach as
    // a zero-length file.
    let err = drop_agent::fetch(port, &s.path("secret/id_ed25519"))
        .expect_err("a key outside the roots must be refused");
    assert!(
        err.to_string().contains("refused"),
        "the refusal must reach the caller: {err}"
    );
}

#[test]
fn test_fetch_fails_fast_when_no_agent_is_listening() {
    // No tunnel is the ordinary case, not an error state: the caller falls
    // back to telling the user how to copy the file across, so this must not
    // hang.
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);

    let started = std::time::Instant::now();
    assert!(drop_agent::fetch(port, "/tmp/whatever.png").is_err());
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "dialling a dead port must fail fast, took {:?}",
        started.elapsed()
    );
}

#[test]
fn test_the_transfer_cap_is_a_real_number() {
    assert_eq!(MAX_TRANSFER_BYTES, 64 * 1024 * 1024);
}

fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let map: std::collections::HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    move |k| map.get(k).cloned()
}

#[test]
fn test_a_declared_port_is_a_declared_tunnel_even_off_ssh() {
    let t = tunnel_from(vars(&[(DROP_PORT_VAR, " 9911 ")])).expect("declared");
    assert_eq!(
        t,
        Tunnel {
            port: 9911,
            declared: true
        }
    );
}

#[test]
fn test_an_ssh_session_without_the_var_probes_the_default_port() {
    for marker in ["SSH_CONNECTION", "SSH_TTY", "SSH_CLIENT"] {
        let t = tunnel_from(vars(&[(marker, "10.0.0.2 51234 10.0.0.1 22")])).expect(marker);
        assert_eq!(
            t,
            Tunnel {
                port: DEFAULT_DROP_PORT,
                declared: false
            },
            "{marker}"
        );
    }
}

#[test]
fn test_no_ssh_and_no_var_means_nothing_to_dial() {
    assert_eq!(tunnel_from(vars(&[("TERM", "xterm-256color")])), None);
    // An empty marker is what an unset-but-exported var looks like.
    assert_eq!(tunnel_from(vars(&[("SSH_CONNECTION", "")])), None);
}

#[test]
fn test_a_malformed_declared_port_falls_back_to_the_probe_rule() {
    let over_ssh = tunnel_from(vars(&[(DROP_PORT_VAR, "eight"), ("SSH_TTY", "/dev/pts/0")]));
    assert_eq!(
        over_ssh,
        Some(Tunnel {
            port: DEFAULT_DROP_PORT,
            declared: false
        })
    );
    assert_eq!(tunnel_from(vars(&[(DROP_PORT_VAR, "eight")])), None);
}
