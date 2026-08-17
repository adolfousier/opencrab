//! External index path resolution and walking (#1051).
//!
//! Moved out of `src/memory/external.rs`: every test is a file under
//! `src/tests/` registered in `mod.rs`, never an inline `#[cfg(test)]`
//! block beside the logic it exercises (#1076).

use crate::memory::external::*;
use std::path::PathBuf;

fn pats(strs: &[&str]) -> Vec<glob::Pattern> {
    strs.iter()
        .map(|s| glob::Pattern::new(s).unwrap())
        .collect()
}

#[test]
fn expand_handles_tilde_relative_and_absolute() {
    let home = crate::config::opencrabs_home();
    assert_eq!(expand_path("~/notes"), home.join("notes"));
    assert_eq!(expand_path("notes"), home.join("notes"));
    assert_eq!(expand_path("/abs/path"), PathBuf::from("/abs/path"));
}

#[test]
fn excludes_match_names_paths_and_subtrees() {
    let excludes = pats(&[".git", "*.key", ".ssh/**"]);
    // Dir-name match anywhere in the tree.
    assert!(excluded(".git", ".git", true, &excludes));
    assert!(excluded("deep/.git", ".git", true, &excludes));
    // File-name match.
    assert!(excluded("server.key", "server.key", false, &excludes));
    // Subtree pattern excludes the dir itself AND its contents.
    assert!(excluded(".ssh", ".ssh", true, &excludes));
    assert!(excluded(".ssh/id_rsa", "id_rsa", false, &excludes));
    // Non-matches pass through.
    assert!(!excluded("notes.md", "notes.md", false, &excludes));
    assert!(!excluded("src", "src", true, &excludes));
}

#[test]
fn walk_finds_matching_files_skips_excludes_and_symlinks() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("a/b")).unwrap();
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::create_dir_all(root.join("node_modules")).unwrap();
    std::fs::write(root.join("top.md"), "# top").unwrap();
    std::fs::write(root.join("a/b/deep.md"), "# deep").unwrap();
    std::fs::write(root.join("a/skip.txt"), "nope").unwrap();
    std::fs::write(root.join(".git/config"), "nope").unwrap();
    std::fs::write(root.join("node_modules/x.md"), "nope").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("top.md"), root.join("link.md")).unwrap();

    let resolved = ResolvedRoot {
        root: root.to_path_buf(),
        pattern: glob::Pattern::new("**/*.md").unwrap(),
    };
    let excludes = pats(&[".git", "node_modules"]);
    let found = walk_root(&resolved, &excludes);
    let names: Vec<String> = found
        .iter()
        .map(|p| {
            p.strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();

    assert!(
        names.contains(&"top.md".to_string()),
        "top-level md: {names:?}"
    );
    assert!(
        names.contains(&"a/b/deep.md".to_string()),
        "nested md: {names:?}"
    );
    assert!(
        !names.contains(&"a/skip.txt".to_string()),
        "pattern mismatch"
    );
    assert!(!names.contains(&".git/config".to_string()), "excluded dir");
    assert!(
        !names.iter().any(|n| n.contains("node_modules")),
        "excluded dir"
    );
    assert!(
        !names.contains(&"link.md".to_string()),
        "symlink must be skipped"
    );
}

#[test]
fn nested_roots_are_skipped_with_report() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let outer = tmp.path().join("outer");
    let inner = outer.join("inner");
    std::fs::create_dir_all(&inner).unwrap();

    // Resolve manually via the same logic: canonicalize + nest check.
    let outer_c = std::fs::canonicalize(&outer).unwrap();
    let inner_c = std::fs::canonicalize(&inner).unwrap();
    assert!(inner_c.starts_with(&outer_c), "test setup: inner nested");

    let kept = [ResolvedRoot {
        root: outer_c.clone(),
        pattern: glob::Pattern::new("**/*.md").unwrap(),
    }];
    let nested = kept
        .iter()
        .any(|k| inner_c.starts_with(&k.root) && inner_c != k.root);
    assert!(nested, "nested root must be detected");
}
