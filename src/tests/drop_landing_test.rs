//! The landed name of a file pulled over the drop tunnel (#1311): the
//! client's own filename, stamped only on collision.

use std::path::{Path, PathBuf};

use crate::utils::drop_landing::{client_file_name, human_size, landing_path, pulled_notice};

fn never(_: &Path) -> bool {
    false
}

#[test]
fn test_keeps_the_clients_filename_when_free() {
    let dir = PathBuf::from("/srv/home/.opencrabs/tmp");
    let got = landing_path(
        &dir,
        "/Users/me/Desktop/Screenshot 2026-09-02.png",
        7,
        never,
    );
    assert_eq!(got, dir.join("Screenshot 2026-09-02.png"));
}

#[test]
fn test_stamps_only_on_collision_and_keeps_the_extension() {
    let dir = PathBuf::from("/tmp/x");
    let taken = |p: &Path| p == dir.join("shot.png");
    let got = landing_path(&dir, "/Users/me/shot.png", 42, taken);
    assert_eq!(got, dir.join("shot-42.png"));
}

#[test]
fn test_collision_on_a_name_without_extension() {
    let dir = PathBuf::from("/tmp/x");
    let got = landing_path(&dir, "/Users/me/README", 9, |_| true);
    assert_eq!(got, dir.join("README-9"));
}

#[test]
fn test_windows_client_paths_split_on_backslash() {
    assert_eq!(
        client_file_name(r"C:\Users\me\Pictures\cat pic.jpg"),
        "cat pic.jpg"
    );
    let dir = PathBuf::from("/tmp/x");
    assert_eq!(
        landing_path(&dir, r"C:\Users\me\a.pdf", 1, never),
        dir.join("a.pdf")
    );
}

#[test]
fn test_unusable_names_fall_back_to_a_placeholder() {
    assert_eq!(client_file_name(""), "dropped-file");
    assert_eq!(client_file_name("/"), "dropped-file");
    assert_eq!(client_file_name("/Users/me/.."), "dropped-file");
    assert_eq!(client_file_name("/Users/me/dir/"), "dir");
}

#[test]
fn test_receipt_names_source_size_and_destination() {
    let n = pulled_notice("shot.png", "/root/.opencrabs/tmp/shot.png", 1536);
    assert_eq!(
        n,
        "Pulled shot.png (1.5 KB) from your machine to /root/.opencrabs/tmp/shot.png"
    );
    assert_eq!(human_size(512), "512 B");
    assert_eq!(human_size(3 * 1024 * 1024), "3.0 MB");
}
