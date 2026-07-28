//! Safety TOMLs reload when the file changes (#851, #852).
//!
//! Both configs were cached in a `OnceLock`, which reads the file once per
//! process. They were described as runtime-editable and were not: adding a
//! blocklist pattern or changing an iteration cap needed a restart, which is
//! the rebuild-free path these files exist to provide.
//!
//! These pin the reload, and equally the caching: re-reading a 5 KB file on
//! every bash call would be its own problem, so the cache must hold when
//! nothing changed.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::brain::tools::toml_hot_reload::HotToml;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Sample {
    #[serde(default)]
    value: u32,
    #[serde(default)]
    name: String,
}

fn tmpdir() -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "opencrabs-hotreload-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&p).expect("tmpdir");
    p
}

/// Write and force a distinct mtime, so the test does not depend on filesystem
/// timestamp granularity.
fn write(path: &std::path::Path, body: &str) {
    std::fs::write(path, body).expect("write");
    // Bump the mtime explicitly. Two writes inside one filesystem timestamp
    // tick would otherwise be indistinguishable, and the test would pass or
    // fail on timer granularity rather than on the cache logic.
    let t = std::time::SystemTime::now() + std::time::Duration::from_secs(1);
    let f = std::fs::File::options()
        .write(true)
        .open(path)
        .expect("reopen to set mtime");
    f.set_modified(t).expect("set mtime");
}

#[test]
fn a_missing_file_yields_none() {
    let path = tmpdir().join("absent.toml");
    let _ = std::fs::remove_file(&path);
    let hot: HotToml<Sample> = HotToml::new(path, "sample");
    assert!(hot.get().is_none());
}

#[test]
fn a_present_file_parses() {
    let path = tmpdir().join("present.toml");
    write(&path, "value = 7\nname = \"alpha\"\n");
    let hot: HotToml<Sample> = HotToml::new(path, "sample");
    let got = hot.get().expect("parses");
    assert_eq!(got.value, 7);
    assert_eq!(got.name, "alpha");
}

#[test]
fn an_edit_is_picked_up_without_a_restart() {
    // The defect: this returned the first value forever.
    let path = tmpdir().join("edited.toml");
    write(&path, "value = 1\n");
    let hot: HotToml<Sample> = HotToml::new(path.clone(), "sample");
    assert_eq!(hot.get().expect("first").value, 1);

    write(&path, "value = 2\n");
    assert_eq!(
        hot.get().expect("second").value,
        2,
        "the edit was not picked up — still serving the cached parse"
    );
}

#[test]
fn an_unchanged_file_serves_the_cache() {
    // Reload must be change-driven, not every-call: the blocklist is consulted
    // on every bash invocation.
    let path = tmpdir().join("stable.toml");
    write(&path, "value = 5\n");
    let hot: HotToml<Sample> = HotToml::new(path, "sample");
    let a = hot.get().expect("first");
    let b = hot.get().expect("second");
    assert!(
        std::sync::Arc::ptr_eq(&a, &b),
        "an unchanged file was re-parsed instead of served from cache"
    );
}

#[test]
fn a_file_appearing_later_is_picked_up() {
    // Absence is cached too, so it must not become permanent.
    let path = tmpdir().join("appears.toml");
    let _ = std::fs::remove_file(&path);
    let hot: HotToml<Sample> = HotToml::new(path.clone(), "sample");
    assert!(hot.get().is_none());

    write(&path, "value = 9\n");
    assert_eq!(
        hot.get().expect("appeared").value,
        9,
        "the file appeared but the cached absence stuck"
    );
}

#[test]
fn a_broken_file_yields_none_and_recovers_when_fixed() {
    // Degrade, never break: these configs add enforcement on top of the
    // hardcoded floor, so an unparseable one must not take the tool down.
    let path = tmpdir().join("broken.toml");
    write(&path, "value = 3\n");
    let hot: HotToml<Sample> = HotToml::new(path.clone(), "sample");
    assert_eq!(hot.get().expect("valid").value, 3);

    write(&path, "value = = = not toml\n");
    assert!(hot.get().is_none(), "a parse error must yield None");

    write(&path, "value = 4\n");
    assert_eq!(
        hot.get().expect("fixed").value,
        4,
        "fixing the file did not re-arm the load"
    );
}
