use crate::tui::onboarding::TEMPLATE_FILES;

#[test]
fn template_files_contains_soul() {
    assert!(
        TEMPLATE_FILES.iter().any(|(name, _)| *name == "SOUL.md"),
        "TEMPLATE_FILES must include SOUL.md"
    );
}

#[test]
fn template_files_contains_code_md() {
    assert!(
        TEMPLATE_FILES.iter().any(|(name, _)| *name == "CODE.md"),
        "TEMPLATE_FILES must include CODE.md (added in 6b4677b)"
    );
}

#[test]
fn template_files_contains_security_md() {
    assert!(
        TEMPLATE_FILES
            .iter()
            .any(|(name, _)| *name == "SECURITY.md"),
        "TEMPLATE_FILES must include SECURITY.md (added in 6b4677b)"
    );
}

#[test]
fn template_files_contains_memory() {
    assert!(
        TEMPLATE_FILES.iter().any(|(name, _)| *name == "MEMORY.md"),
        "TEMPLATE_FILES must include MEMORY.md"
    );
}

#[test]
fn template_files_all_have_content() {
    for (name, content) in TEMPLATE_FILES {
        assert!(
            !content.trim().is_empty(),
            "Template {} must have non-empty content",
            name
        );
    }
}

#[test]
fn brain_files_in_memory_index_contains_code() {
    // Memory indexer's BRAIN_FILES array (src/memory/index.rs)
    // must include CODE.md so it gets indexed for semantic search.
    use crate::memory::BRAIN_FILES;
    assert!(
        BRAIN_FILES.contains(&"CODE.md"),
        "Memory index BRAIN_FILES must include CODE.md"
    );
}

#[test]
fn brain_files_in_memory_index_contains_security() {
    use crate::memory::BRAIN_FILES;
    assert!(
        BRAIN_FILES.contains(&"SECURITY.md"),
        "Memory index BRAIN_FILES must include SECURITY.md"
    );
}

// --- Seeding guards (#989) -------------------------------------------------
//
// The per-file assertions above are the pattern that let HEARTBEAT.md slip:
// each one names a file, so a template nobody wrote an assertion for is
// invisible. These derive the expected set instead of listing it.

/// Markdown templates the upstream sync path tracks, by local filename.
fn tracked_markdown() -> Vec<&'static str> {
    use crate::brain::rsi_sync::{TRACKED_FOR_TEST, TemplateKind};
    TRACKED_FOR_TEST
        .iter()
        .filter(|t| t.kind == TemplateKind::Markdown)
        .map(|t| t.local)
        .collect()
}

#[test]
fn seeding_creates_every_tracked_markdown_template() {
    // rsi_sync tracked 9 markdown templates while profile creation seeded 8,
    // so HEARTBEAT.md reached existing installs through sync and never
    // reached a fresh profile.
    let dir = tempfile::tempdir().expect("tempdir");
    crate::config::profile::seed_brain_templates(dir.path());

    for name in tracked_markdown() {
        assert!(
            dir.path().join(name).exists(),
            "{name} is tracked for upstream sync but a fresh profile never gets one"
        );
    }
}

#[test]
fn the_two_seed_lists_stay_in_lockstep() {
    // profile.rs seeds on profile creation, the TUI seeds during onboarding.
    // A file added to one and missed by the other means the workspace you get
    // depends on which door you came through.
    let dir = tempfile::tempdir().expect("tempdir");
    crate::config::profile::seed_brain_templates(dir.path());

    for (name, _) in TEMPLATE_FILES {
        assert!(
            dir.path().join(name).exists(),
            "{name} is seeded by onboarding but not by profile creation"
        );
    }
}

#[test]
fn seeding_never_overwrites_an_existing_file() {
    // The whole upstreaming model rests on this: a user's edited brain file is
    // never clobbered by a later seed pass.
    let dir = tempfile::tempdir().expect("tempdir");
    let mine = dir.path().join("SOUL.md");
    std::fs::write(&mine, "# mine, hand written").expect("write");

    crate::config::profile::seed_brain_templates(dir.path());

    assert_eq!(
        std::fs::read_to_string(&mine).expect("reads"),
        "# mine, hand written",
        "seeding overwrote an existing brain file"
    );
}
