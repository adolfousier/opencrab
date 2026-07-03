//! Tests for scraped-markdown export. The directory resolution is DB- and
//! profile-aware and exercised end-to-end, so here we pin the pure filename
//! derivation (host+path -> safe `.md` stem) and prove a write round-trips to
//! the resolved path.

use crate::brain::tools::web_scrape::export::{filename_for_url, write_markdown};

#[test]
fn derives_host_and_path_filename() {
    assert_eq!(
        filename_for_url("https://example.com/blog/post-1"),
        "example.com-blog-post-1.md"
    );
}

#[test]
fn root_url_gets_index_stem() {
    // No path beyond "/" — the host alone names the file, so a bare domain still
    // yields a sensible, non-empty stem.
    assert_eq!(filename_for_url("https://example.com/"), "example.com.md");
}

#[test]
fn collapses_runs_of_unsafe_chars() {
    // The name comes from host + path (port and query live elsewhere on the
    // URL and are dropped); repeated path separators collapse to single dashes
    // with dots preserved for readability and no trailing dash.
    let name = filename_for_url("https://sub.example.com:8443/a//b?x=1&y=2");
    assert_eq!(name, "sub.example.com-a-b.md");
}

#[test]
fn unparsable_url_still_names_a_file() {
    let name = filename_for_url("not a url");
    assert!(name.ends_with(".md"));
    assert!(!name.is_empty());
}

#[tokio::test]
async fn write_markdown_round_trips_to_dir() {
    let dir = std::env::temp_dir().join(format!("web_scrape_export_{}", uuid::Uuid::new_v4()));
    let written = write_markdown(&dir, "https://example.com/page", "# Hello\n\nbody")
        .await
        .expect("write should succeed");

    assert_eq!(written, dir.join("example.com-page.md"));
    let back = std::fs::read_to_string(&written).expect("file should exist");
    assert_eq!(back, "# Hello\n\nbody");

    let _ = std::fs::remove_dir_all(&dir);
}
