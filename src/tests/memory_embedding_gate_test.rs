//! #1062: a daemon's `write_opencrabs_file` hung forever because the embedding
//! API client had no HTTP timeout and the write path awaited it inline, once
//! per chunk. Three pins:
//!
//! 1. `embedding_api_configured` folds in `vector_enabled`, so `false` means
//!    no embedding work at all regardless of a leftover `[memory.embedding]`
//!    section (previously only the local path checked the flag).
//! 2. `embed_content_api` is a no-op when vector search is off, even with a
//!    fully configured API section pointing at a live socket.
//! 3. `index_file` returns promptly even when the embedding endpoint
//!    blackholes (accepts TCP, never responds). Before the fix this awaited
//!    the embedding task inline, so the write tool never returned. Phase 2
//!    is now detached; FTS (Phase 1) completes and the tool is free.

use crate::config::profile::with_profile_home_async;
use crate::memory::embedding::{embed_content_api, embed_via_api};
use crate::memory::{embedding_api_configured, get_store, index_file};
use std::io::Write;
use std::time::Duration;

/// A TCP endpoint that accepts connections and never responds: reqwest
/// connects instantly, then waits forever for bytes. This is the exact shape
/// of the endpoint that wedged the daemon.
fn spawn_blackhole_endpoint() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind blackhole");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            // Hold the connection open, say nothing, close nothing.
            drop(stream);
        }
    });
    port
}

fn profile_config() -> String {
    let port = spawn_blackhole_endpoint();
    format!(
        "[memory]\nvector_enabled = {{vector}}\n\n[memory.embedding]\nurl = \"http://127.0.0.1:{port}/v1\"\nmodel = \"test-embed\"\n"
    )
}

fn write_profile_config(profile: &str, vector: bool) {
    let _ = profile; // home resolution happens via with_profile_home_async
    let home = crate::config::opencrabs_home();
    std::fs::create_dir_all(&home).expect("create profile home");
    let cfg = profile_config().replace("{vector}", if vector { "true" } else { "false" });
    std::fs::write(home.join("config.toml"), cfg).expect("write config.toml");
}

#[tokio::test]
async fn vector_off_disables_api_embedding_entirely() {
    let p = format!("gate-off-{}", uuid::Uuid::new_v4());
    with_profile_home_async(Some(&p), async {
        write_profile_config(&p, false);
        assert!(
            !embedding_api_configured(),
            "vector_enabled = false must win over a fully configured [memory.embedding] section"
        );
    })
    .await;
}

#[tokio::test]
async fn vector_on_with_api_section_still_configures() {
    let p = format!("gate-on-{}", uuid::Uuid::new_v4());
    with_profile_home_async(Some(&p), async {
        write_profile_config(&p, true);
        assert!(
            embedding_api_configured(),
            "vector on + url + model is the legitimate API path and must stay enabled"
        );
    })
    .await;
}

#[tokio::test]
async fn embed_content_api_is_a_noop_when_vector_off() {
    let p = format!("noop-{}", uuid::Uuid::new_v4());
    with_profile_home_async(Some(&p), async {
        write_profile_config(&p, false);
        let store = get_store().expect("store opens");
        // Before the gate this fired one HTTP call per chunk at the
        // blackhole. With the gate it must return Ok without any HTTP.
        let started = std::time::Instant::now();
        let res = embed_content_api(store, "line one\nline two\nline three\n").await;
        assert!(res.is_ok(), "gated call returns Ok: {res:?}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "no-op gate must be immediate, took {:?}",
            started.elapsed()
        );
    })
    .await;
}

#[tokio::test]
async fn index_file_returns_promptly_when_endpoint_blackholes() {
    let p = format!("blackhole-{}", uuid::Uuid::new_v4());
    with_profile_home_async(Some(&p), async {
        write_profile_config(&p, true);
        let store = get_store().expect("store opens");
        let dir = std::env::temp_dir().join(format!("oc-1062-{p}"));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let file = dir.join("note.md");
        let mut f = std::fs::File::create(&file).expect("create note");
        writeln!(f, "body that changes the content hash each run {p}").expect("write body");
        drop(f);

        let started = std::time::Instant::now();
        let res = index_file(store, &file).await;
        assert!(res.is_ok(), "index_file must succeed: {res:?}");
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "#1062 regression: index_file blocked on the blackholed embedding endpoint \
             for {:?}; Phase 2 must be detached",
            started.elapsed()
        );
        let _ = std::fs::remove_dir_all(&dir);
    })
    .await;
}

#[tokio::test]
async fn embed_via_api_errors_on_connection_refusal_instead_of_hanging() {
    let p = format!("refused-{}", uuid::Uuid::new_v4());
    with_profile_home_async(Some(&p), async {
        // Port 1 on loopback: nothing listens, connection refused immediately.
        // Pins that request errors surface as Err rather than an infinite
        // await; the hardcoded 60s timeout itself is not unit-testable fast.
        write_profile_config(&p, true);
        let cfg_home = crate::config::opencrabs_home();
        let raw = std::fs::read_to_string(cfg_home.join("config.toml")).unwrap();
        let raw = raw.replace(&format!("127.0.0.1:{}", extract_port(&raw)), "127.0.0.1:1");
        std::fs::write(cfg_home.join("config.toml"), raw).expect("rewrite config");

        let started = std::time::Instant::now();
        let res = embed_via_api("probe").await;
        assert!(res.is_err(), "refused endpoint must surface Err: {res:?}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "error path must be fast, took {:?}",
            started.elapsed()
        );
    })
    .await;
}

fn extract_port(cfg: &str) -> String {
    let start = cfg.find("127.0.0.1:").expect("endpoint present") + "127.0.0.1:".len();
    let end = cfg[start..].find('/').expect("path separator") + start;
    cfg[start..end].to_string()
}
