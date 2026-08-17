//! External-path freshness sweep (#1051).
//!
//! Moved out of `src/memory/external_sweep.rs`: tests live under
//! `src/tests/`, never inline beside the logic (#1076).

use crate::memory::db::Store;
use crate::memory::external::ResolvedRoot;
use crate::memory::external_sweep::*;
use glob::Pattern;
use std::path::Path;

fn md_root(path: &Path) -> ResolvedRoot {
    ResolvedRoot {
        root: path.to_path_buf(),
        pattern: Pattern::new("**/*.md").unwrap(),
    }
}

#[test]
fn cold_sweep_indexes_then_warm_sweep_reads_nothing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_path_buf();
    std::fs::write(root.join("a.md"), "# a").unwrap();
    let store = Store::open(&tmp.path().join("mem.db")).expect("store");

    let roots = vec![md_root(&root)];
    let excludes = vec![];

    let (r1, dirs1) = sweep_external_with(&store, &roots, &excludes, None);
    assert_eq!(r1.indexed, 1, "cold pass indexes the one file");
    assert_eq!(r1.on_disk, 1);
    assert_eq!(r1.pruned, 0);

    let (r2, _dirs2) = sweep_external_with(&store, &roots, &excludes, Some(&dirs1));
    assert_eq!(r2.reads, 0, "quiet tree reads nothing");
    assert_eq!(r2.indexed, 0, "quiet tree indexes nothing");
    assert_eq!(r2.pruned, 0);
    assert_eq!(r2.on_disk, 1);
}

#[test]
fn warm_sweep_discovers_additions_and_prunes_deletions() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_path_buf();
    std::fs::write(root.join("a.md"), "# a").unwrap();
    let store = Store::open(&tmp.path().join("mem.db")).expect("store");

    let roots = vec![md_root(&root)];
    let excludes = vec![];

    let (_r1, dirs1) = sweep_external_with(&store, &roots, &excludes, None);

    // Add b.md and delete a.md — both bump the root dir's mtime.
    std::fs::write(root.join("b.md"), "# b").unwrap();
    std::fs::remove_file(root.join("a.md")).unwrap();

    let (r2, _dirs2) = sweep_external_with(&store, &roots, &excludes, Some(&dirs1));
    assert_eq!(r2.indexed, 1, "b.md discovered and indexed");
    assert_eq!(r2.pruned, 1, "deleted a.md pruned");
    assert_eq!(r2.on_disk, 1);
}

#[test]
fn removing_a_config_path_prunes_its_documents() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root_a = tmp.path().join("a");
    let root_b = tmp.path().join("b");
    std::fs::create_dir_all(&root_a).unwrap();
    std::fs::create_dir_all(&root_b).unwrap();
    std::fs::write(root_a.join("x.md"), "# x").unwrap();
    std::fs::write(root_b.join("y.md"), "# y").unwrap();
    let store = Store::open(&tmp.path().join("mem.db")).expect("store");

    let excludes = vec![];
    let both = vec![md_root(&root_a), md_root(&root_b)];

    let (r1, dirs1) = sweep_external_with(&store, &both, &excludes, None);
    assert_eq!(r1.indexed, 2, "both roots indexed cold");

    // Config now only lists root_a: root_b's doc must be pruned.
    let only_a = vec![md_root(&root_a)];
    let (r2, _dirs2) = sweep_external_with(&store, &only_a, &excludes, Some(&dirs1));
    assert_eq!(r2.pruned, 1, "root_b doc pruned after config removal");
    assert_eq!(r2.on_disk, 1);
}

#[test]
fn excludes_are_respected_by_the_sweep() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_path_buf();
    std::fs::create_dir_all(root.join("node_modules")).unwrap();
    std::fs::write(root.join("keep.md"), "# keep").unwrap();
    std::fs::write(root.join("node_modules/skip.md"), "# skip").unwrap();
    let store = Store::open(&tmp.path().join("mem.db")).expect("store");

    let roots = vec![md_root(&root)];
    let excludes = vec![Pattern::new("node_modules").unwrap()];

    let (r1, _dirs) = sweep_external_with(&store, &roots, &excludes, None);
    assert_eq!(r1.indexed, 1, "only keep.md indexed");
    assert_eq!(r1.on_disk, 1);
}
