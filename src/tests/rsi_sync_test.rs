//! RSI Template Sync Tests
//!
//! Tests for the upstream brain file template synchronization feature.
//! Verifies version gating, section extraction, state persistence, and backup safety.

use crate::brain::rsi_sync::{
    SyncState, content_fingerprint, extract_new_sections, extract_section_headers,
    upstream_changed, version_changed,
};
use std::collections::HashMap;

// --- Section Extraction Tests ---

#[test]
fn extracts_top_level_sections() {
    let content = "## Section A\nSome content\n## Section B\nMore content\n";
    let headers = extract_section_headers(content);
    assert_eq!(headers, vec!["## Section A", "## Section B"]);
}

#[test]
fn extracts_subsections() {
    let content = "## Section A\n### Sub A1\n### Sub A2\n## Section B\n";
    let headers = extract_section_headers(content);
    assert_eq!(
        headers,
        vec!["## Section A", "### Sub A1", "### Sub A2", "## Section B"]
    );
}

#[test]
fn ignores_non_header_lines() {
    let content = "Some text\n## Real Header\n- bullet point\n### Sub Header\n";
    let headers = extract_section_headers(content);
    assert_eq!(headers, vec!["## Real Header", "### Sub Header"]);
}

#[test]
fn handles_empty_content() {
    let headers = extract_section_headers("");
    assert!(headers.is_empty());
}

#[test]
fn extract_new_sections_all_new() {
    let local = "# Title\n\n## Existing\nOld content";
    let upstream = "# Title\n\n## Existing\nOld content\n\n## New Section\nNew content here";
    let new = extract_new_sections(local, upstream);
    assert!(new.contains("## New Section"));
    assert!(new.contains("New content here"));
    assert!(!new.contains("## Existing"));
}

#[test]
fn extract_new_sections_none_new() {
    let local = "# Title\n\n## Section A\nContent";
    let upstream = "# Title\n\n## Section A\nContent";
    let new = extract_new_sections(local, upstream);
    assert!(new.trim().is_empty());
}

#[test]
fn extract_new_sections_partial_overlap() {
    let local = "# Title\n\n## Shared\nLocal version\n\n## Local Only\nStuff";
    let upstream = "# Title\n\n## Shared\nUpstream version\n\n## Upstream Only\nNew stuff\n\n### Sub Detail\nMore";
    let new = extract_new_sections(local, upstream);
    assert!(!new.contains("## Shared"));
    assert!(new.contains("## Upstream Only"));
    assert!(new.contains("### Sub Detail"));
}

// --- Version Gate Tests ---

#[test]
fn version_change_is_detected() {
    // Still reported and logged; it just no longer gates whether to look
    // (#820).
    let state = SyncState {
        last_synced_version: "0.3.13".to_string(),
        ..Default::default()
    };
    assert!(version_changed(&state));
}

#[test]
fn same_version_is_not_a_change() {
    let state = SyncState {
        last_synced_version: crate::VERSION.to_string(),
        ..Default::default()
    };
    assert!(!version_changed(&state));
}

// --- Content gate (#820) ---
// Version equality asked "was the app upgraded" when the question is "is
// upstream different from mine". Those diverge whenever a template is fixed
// after a release: #816 and #817 landed ~21 hours after the v0.3.75 bump and
// were undeliverable until the next one.

#[test]
fn unchanged_upstream_is_not_worth_syncing() {
    // The critical property: nothing changed means do nothing. No merge, no
    // backup, no write.
    let upstream = "# Notes\n\n## A\n\nbody\n";
    let mut state = SyncState::default();
    state
        .file_hashes
        .insert("TOOLS.md".to_string(), content_fingerprint(upstream));

    assert!(!upstream_changed(&state, "TOOLS.md", upstream));
}

#[test]
fn a_single_byte_makes_it_worth_syncing() {
    let mut state = SyncState::default();
    state
        .file_hashes
        .insert("TOOLS.md".to_string(), content_fingerprint("body\n"));

    assert!(upstream_changed(&state, "TOOLS.md", "body!\n"));
}

#[test]
fn a_never_seen_file_counts_as_changed() {
    // No record means never synced, so a first run must still merge.
    assert!(upstream_changed(
        &SyncState::default(),
        "TOOLS.md",
        "anything"
    ));
}

#[test]
fn each_file_is_gated_independently() {
    // One file changing must not drag an unchanged one into a rewrite.
    let mut state = SyncState::default();
    state
        .file_hashes
        .insert("TOOLS.md".to_string(), content_fingerprint("same"));
    state
        .file_hashes
        .insert("CODE.md".to_string(), content_fingerprint("old"));

    assert!(!upstream_changed(&state, "TOOLS.md", "same"));
    assert!(upstream_changed(&state, "CODE.md", "new"));
}

#[test]
fn identical_content_hashes_identically() {
    // A fingerprint rather than a timestamp because a file can be rewritten
    // with identical content by a rebase or reformat.
    assert_eq!(content_fingerprint("abc"), content_fingerprint("abc"));
    assert_ne!(content_fingerprint("abc"), content_fingerprint("abd"));
}

// --- State Persistence Tests ---

#[test]
fn state_tracks_per_file_dates() {
    let mut file_dates: HashMap<String, String> = HashMap::new();
    file_dates.insert("SOUL.md".to_string(), "2026-04-27T21:00:00Z".to_string());
    file_dates.insert("TOOLS.md".to_string(), "2026-04-27T21:00:00Z".to_string());

    assert_eq!(file_dates.len(), 2);
    assert!(file_dates.contains_key("SOUL.md"));
    assert!(file_dates.contains_key("TOOLS.md"));
    assert!(!file_dates.contains_key("MEMORY.md"));
}

#[test]
fn sync_state_parse() {
    let toml = r#"
last_synced_version = "0.3.14"
last_sync_date = "2026-04-27T21:00:00Z"

[files]
SOUL.md = "2026-04-27T21:00:00Z"
TOOLS.md = "2026-04-27T21:00:00Z"
"#;
    let mut state = SyncState::default();
    let mut in_files = false;
    for line in toml.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed == "[files]" {
            in_files = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_files = false;
            continue;
        }
        if let Some((k, v)) = trimmed.split_once('=') {
            let key = k.trim();
            let val = v.trim().trim_matches('"');
            if in_files {
                state.file_dates.insert(key.to_string(), val.to_string());
            } else if key == "last_synced_version" {
                state.last_synced_version = val.to_string();
            } else if key == "last_sync_date" {
                state.last_sync_date = val.to_string();
            }
        }
    }

    assert_eq!(state.last_synced_version, "0.3.14");
    assert_eq!(state.last_sync_date, "2026-04-27T21:00:00Z");
    assert_eq!(state.file_dates.len(), 2);
    assert_eq!(
        state.file_dates.get("SOUL.md").unwrap(),
        "2026-04-27T21:00:00Z"
    );
}

// --- Backup Safety Tests ---

#[test]
fn backup_filename_format() {
    let filename = "SOUL.md";
    let timestamp = "2026-04-27T210000";
    let backup_name = format!("{}.{}.bak", filename, timestamp);
    assert_eq!(backup_name, "SOUL.md.2026-04-27T210000.bak");
    assert!(backup_name.ends_with(".bak"));
}

#[test]
fn backup_preserves_original_extension() {
    let filename = "TOOLS.md";
    let backup_name = format!("{}.bak", filename);
    assert!(backup_name.contains("TOOLS.md"));
}

// --- Append-Only Safety Tests ---

#[test]
fn append_increases_content_length() {
    let original = "## Existing Section\nUser content here\n";
    let new_section = "\n## New Section\nUpstream content\n";
    let merged = format!("{}{}", original, new_section);
    assert!(merged.len() > original.len());
    assert!(merged.contains("User content here"));
    assert!(merged.contains("Upstream content"));
}

#[test]
fn append_never_removes_existing_content() {
    let original = "## Hard Rules\n- Rule 1\n- Rule 2\n";
    let new_section = "\n## New Rules\n- Rule 3\n";
    let merged = format!("{}{}", original, new_section);
    assert!(merged.contains("Rule 1"));
    assert!(merged.contains("Rule 2"));
    assert!(merged.contains("Rule 3"));
}
