//! Tests for the tmp attachment naming that leads with the original filename
//! while keeping the `{chat_id}-{ts}` suffix for origin detection and
//! collision-safety (#513).

use crate::channels::telegram::media::{
    attachment_tmp_name, channel_attachments_dir, migrate_flat_attachments_in,
    sanitize_filename_component,
};

#[test]
fn attachments_dir_is_profile_resolved_via_opencrabs_home() {
    // #681: the store must be anchored on opencrabs_home() (profile-aware), not
    // a hardcoded ~/.opencrabs/ root, so a named-profile instance writes under
    // its own home and matches what push_known_paths tells the agent.
    let dir = channel_attachments_dir();
    assert_eq!(
        dir,
        crate::config::opencrabs_home().join("channel_attachments"),
        "attachments dir must route through opencrabs_home()"
    );
    assert!(dir.ends_with("channel_attachments"));
}

#[test]
fn document_name_leads_with_original_stem() {
    // The readable name leads; chat_id and ts trail; the real extension is kept.
    let out = attachment_tmp_name(
        Some("plan_mode_feature_de5e97b3.plan.md"),
        "doc",
        -1003,
        1783,
        "bin",
    );
    assert_eq!(out, "plan_mode_feature_de5e97b3.plan--1003-1783.md");
    assert!(out.starts_with("plan_mode_feature_de5e97b3.plan"));
    assert!(out.ends_with("-1003-1783.md"));
}

#[test]
fn missing_filename_falls_back_to_synthetic_stem() {
    // No filename → the old synthetic stem, so behavior is unchanged for blobs.
    let out = attachment_tmp_name(None, "doc", 42, 99, "bin");
    assert_eq!(out, "doc-42-99.bin");
}

#[test]
fn dotless_filename_uses_fallback_extension() {
    // A name with no extension keeps the name as the stem and takes the default.
    let out = attachment_tmp_name(Some("README"), "doc", 7, 8, "bin");
    assert_eq!(out, "README-7-8.bin");
}

#[test]
fn unsafe_characters_in_name_are_sanitized() {
    // Spaces and slashes collapse to underscores; dots/dashes/underscores stay.
    let out = attachment_tmp_name(Some("my report v2/final.txt"), "doc", 1, 2, "bin");
    assert_eq!(out, "my_report_v2_final-1-2.txt");
}

#[test]
fn audio_uses_the_same_scheme() {
    let out = attachment_tmp_name(Some("song.final.mp3"), "audio", 5, 6, "ogg");
    assert_eq!(out, "song.final-5-6.mp3");
}

#[test]
fn sanitize_never_yields_empty() {
    // A component made entirely of unsafe characters must not vanish.
    assert_eq!(sanitize_filename_component("///"), "file");
    assert_eq!(sanitize_filename_component("  "), "file");
    assert_eq!(sanitize_filename_component("a.b-c_d"), "a.b-c_d");
}

#[test]
fn migration_sweeps_flat_files_into_platform_subdir() {
    let base = tempfile::tempdir().expect("tempdir");
    let base = base.path();
    // Two loose pre-subdir attachments and an unrelated existing subdir.
    std::fs::write(base.join("uuid_report.md"), b"a").unwrap();
    std::fs::write(base.join("uuid_photo.jpg"), b"b").unwrap();
    std::fs::create_dir_all(base.join("slack")).unwrap();
    std::fs::write(base.join("slack").join("keep.txt"), b"c").unwrap();

    let moved = migrate_flat_attachments_in(base, "telegram");
    assert_eq!(moved, 2, "both loose files move");

    // Loose files now live under telegram/, originals gone from the root.
    assert!(base.join("telegram").join("uuid_report.md").exists());
    assert!(base.join("telegram").join("uuid_photo.jpg").exists());
    assert!(!base.join("uuid_report.md").exists());
    // The pre-existing platform subdir is untouched.
    assert!(base.join("slack").join("keep.txt").exists());

    // Idempotent: a second run finds nothing loose to move.
    assert_eq!(migrate_flat_attachments_in(base, "telegram"), 0);
}

#[test]
fn migration_keeps_original_on_name_clash() {
    let base = tempfile::tempdir().expect("tempdir");
    let base = base.path();
    std::fs::create_dir_all(base.join("telegram")).unwrap();
    // A file already exists at the destination name.
    std::fs::write(base.join("telegram").join("dup.md"), b"existing").unwrap();
    std::fs::write(base.join("dup.md"), b"new").unwrap();

    let moved = migrate_flat_attachments_in(base, "telegram");
    assert_eq!(moved, 0, "clashing file is not moved");
    // Both the destination and the loose original survive — nothing clobbered.
    assert_eq!(
        std::fs::read(base.join("telegram").join("dup.md")).unwrap(),
        b"existing"
    );
    assert!(base.join("dup.md").exists());
}
