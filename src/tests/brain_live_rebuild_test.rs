//! Live system-brain rebuild (#213).
//!
//! The system brain was historically assembled once at startup and cached as a
//! static string, so edits to brain files (manual, `write_opencrabs_file`,
//! `self_improve`) stayed invisible until the process restarted. `BrainRebuild`
//! fixes that: it returns the startup seed verbatim while nothing changes (so
//! the provider prompt cache stays warm), and rebuilds from disk on the next
//! turn once a brain file's mtime advances.
//!
//! These tests drive `BrainRebuild` directly with deterministic mtimes (set via
//! `File::set_modified`, no sleeping) so they can't flake on filesystem clock
//! granularity.

use crate::brain::agent::BrainRebuild;
use crate::brain::prompt_builder::BrainLoader;
use std::time::{Duration, SystemTime};
use tempfile::TempDir;

/// A fixed, well-past instant so the seeded mtime is deterministic.
fn t0() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000)
}

fn write_with_mtime(path: &std::path::Path, content: &str, mtime: SystemTime) {
    std::fs::write(path, content).expect("write brain file");
    std::fs::File::options()
        .write(true)
        .open(path)
        .expect("open for mtime")
        .set_modified(mtime)
        .expect("set mtime");
}

#[test]
fn render_returns_seed_until_a_brain_file_changes() {
    let dir = TempDir::new().unwrap();
    let soul = dir.path().join("SOUL.md");
    write_with_mtime(&soul, "--- SOUL.md ---\noriginal personality\n", t0());

    let loader = BrainLoader::new(dir.path().to_path_buf());
    // Seed with a sentinel that the real assembly would never produce, so we
    // can prove the seed is returned verbatim on the warm path.
    let rebuild = BrainRebuild::new(
        loader,
        None,
        true,  // core brain
        false, // no lazy-tools suffix
        "SEED-VERBATIM".to_string(),
    );

    // Nothing changed since construction → exact seed, no disk rebuild.
    assert_eq!(rebuild.render(), "SEED-VERBATIM");
    assert_eq!(rebuild.render(), "SEED-VERBATIM", "still warm on repeat");

    // Edit the brain file and advance its mtime → next render rebuilds.
    write_with_mtime(
        &soul,
        "--- SOUL.md ---\nEDITED_PERSONALITY_MARKER\n",
        t0() + Duration::from_secs(60),
    );

    let rebuilt = rebuild.render();
    assert_ne!(rebuilt, "SEED-VERBATIM", "stale seed must be dropped");
    assert!(
        rebuilt.contains("EDITED_PERSONALITY_MARKER"),
        "rebuilt brain must reflect the on-disk edit, got:\n{rebuilt}"
    );
}

#[test]
fn rebuild_is_cached_after_a_change_until_the_next_change() {
    let dir = TempDir::new().unwrap();
    let soul = dir.path().join("SOUL.md");
    write_with_mtime(&soul, "--- SOUL.md ---\nv1\n", t0());

    let loader = BrainLoader::new(dir.path().to_path_buf());
    let rebuild = BrainRebuild::new(loader, None, true, false, "SEED".to_string());

    // Trigger a rebuild.
    write_with_mtime(
        &soul,
        "--- SOUL.md ---\nMARKER_V2\n",
        t0() + Duration::from_secs(60),
    );
    let first = rebuild.render();
    assert!(first.contains("MARKER_V2"));

    // No further mtime change → identical render returned from cache (warm
    // prompt cache, byte-for-byte stable).
    assert_eq!(rebuild.render(), first, "no change → cached render reused");
}

#[test]
fn no_rebuild_handle_falls_back_to_static_brain() {
    // A loader pointed at an empty dir, used only to prove the seed survives
    // when nothing changes — the static-brain fallback path is exercised by
    // the existing agent tests via `default_system_brain`; here we assert the
    // seed is what render hands back when the dir has no newer files.
    let dir = TempDir::new().unwrap();
    let loader = BrainLoader::new(dir.path().to_path_buf());
    let rebuild = BrainRebuild::new(loader, None, true, false, "ONLY-SEED".to_string());
    assert_eq!(rebuild.render(), "ONLY-SEED");
}
