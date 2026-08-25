//! #1169 regression tests: self-describing file-not-found errors.
//!
//! Covers the shared chokepoint `validate_file_path` used by read, edit and
//! hashline-edit: markdown-wrapped paths resolve transparently, misses in
//! populated directories carry fuzzy suggestions, truly-missing directories
//! keep the plain error.

use crate::brain::tools::error::{strip_path_wrappers, validate_file_path};
use std::fs;
use std::path::PathBuf;

fn fixture_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("opencrabs_1169_{}_{}", tag, std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create fixture dir");
    dir
}

#[test]
fn bold_wrapped_path_resolves() {
    let dir = fixture_dir("bold");
    fs::write(dir.join("f.rs"), "fn main() {}").expect("seed");
    let wrapped = format!("**{}**", dir.join("f.rs").display());
    let got = validate_file_path(&wrapped, &dir).expect("wrapped path must resolve");
    assert_eq!(got, dir.join("f.rs"));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn backtick_wrapped_path_resolves() {
    let dir = fixture_dir("backtick");
    fs::write(dir.join("g.txt"), "hi").expect("seed");
    let wrapped = format!("`{}`", dir.join("g.txt").display());
    let got = validate_file_path(&wrapped, &dir).expect("backticked path must resolve");
    assert_eq!(got, dir.join("g.txt"));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn suggestion_appears_for_wrong_filename_in_populated_dir() {
    let dir = fixture_dir("fuzzy");
    fs::write(dir.join("existing_file.rs"), "x").expect("seed");
    fs::write(dir.join("other_file.rs"), "y").expect("seed");
    let err = validate_file_path("existin_file.rs", &dir).expect_err("must miss");
    assert!(
        err.contains("did you mean"),
        "expected suggestion hint, got: {err}"
    );
    assert!(
        err.contains("existing_file.rs"),
        "expected closest match named, got: {err}"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn truly_missing_directory_keeps_plain_error() {
    let dir = fixture_dir("orphan");
    let missing = dir.join("absent_subdir").join("x.rs");
    let err = validate_file_path(&missing.display().to_string(), &dir).expect_err("must miss");
    assert!(err.starts_with("File not found:"), "got: {err}");
    assert!(
        !err.contains("did you mean"),
        "no suggestions from a missing parent, got: {err}"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn strip_path_wrappers_cases() {
    assert_eq!(strip_path_wrappers("**/tmp/f.rs**"), "/tmp/f.rs");
    assert_eq!(strip_path_wrappers("`/tmp/f.rs`"), "/tmp/f.rs");
    assert_eq!(strip_path_wrappers("\"/tmp/f.rs\""), "/tmp/f.rs");
    assert_eq!(strip_path_wrappers("/tmp/f.rs"), "/tmp/f.rs");
    assert_eq!(strip_path_wrappers("  /tmp/f.rs  "), "/tmp/f.rs");
    assert_eq!(strip_path_wrappers("**unbalanced"), "**unbalanced");
}
