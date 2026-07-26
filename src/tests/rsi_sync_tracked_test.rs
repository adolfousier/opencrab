//! What the template sync tracks, and how each entry routes (#823).
//!
//! The failure mode here is silence. A wrong upstream path yields a 404, which
//! the sync treats as an ordinary fetch failure and skips quietly, so it looks
//! exactly like "nothing to sync". #816 and #817 sat undeliverable for a week
//! without producing a single error, which is why these assertions exist
//! rather than trusting the paths by eye.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::brain::rsi_sync::{TRACKED_FOR_TEST, TemplateKind};

#[test]
fn config_examples_are_tracked() {
    // The whole point of #819: without these, a pricing fix reaches the
    // repository and never reaches a user.
    for name in [
        "usage_pricing.toml",
        "config.toml",
        "commands.toml",
        "tools.toml",
        "rtk_filters.toml",
    ] {
        assert!(
            TRACKED_FOR_TEST.iter().any(|t| t.local == name),
            "{name} must be tracked or its upstream fixes cannot be delivered"
        );
    }
}

#[test]
fn keys_toml_is_never_tracked() {
    // It holds credentials and its example is placeholders, so syncing it
    // would push dummy keys into a working install. Asserted rather than left
    // to a comment, since a future edit adding it back would be silent.
    assert!(
        !TRACKED_FOR_TEST.iter().any(|t| t.local == "keys.toml"),
        "keys.toml holds credentials and must never be merged from upstream"
    );
}

#[test]
fn brain_files_resolve_under_the_templates_directory() {
    let soul = TRACKED_FOR_TEST
        .iter()
        .find(|t| t.local == "SOUL.md")
        .expect("SOUL.md must be tracked");
    assert_eq!(soul.upstream, "src/docs/reference/templates/SOUL.md");
    assert_eq!(soul.kind, TemplateKind::Markdown);
}

#[test]
fn config_examples_resolve_to_the_repo_root_with_the_example_suffix() {
    // Local `usage_pricing.toml`, upstream `usage_pricing.toml.example`. Get
    // this wrong and it 404s silently, which is indistinguishable from
    // "nothing to sync".
    let pricing = TRACKED_FOR_TEST
        .iter()
        .find(|t| t.local == "usage_pricing.toml")
        .expect("usage_pricing.toml must be tracked");
    assert_eq!(pricing.upstream, "usage_pricing.toml.example");
    assert_eq!(pricing.kind, TemplateKind::Toml);
}

#[test]
fn every_toml_entry_routes_through_the_key_merge() {
    // Sending a config down the markdown path appends a duplicate table and
    // the file stops parsing, which for usage_pricing.toml takes the user's
    // pricing config offline.
    for t in TRACKED_FOR_TEST
        .iter()
        .filter(|t| t.local.ends_with(".toml"))
    {
        assert_eq!(
            t.kind,
            TemplateKind::Toml,
            "{} must merge by key, not by appended section",
            t.local
        );
    }
}

#[test]
fn every_markdown_entry_routes_through_the_section_merge() {
    for t in TRACKED_FOR_TEST.iter().filter(|t| t.local.ends_with(".md")) {
        assert_eq!(t.kind, TemplateKind::Markdown, "{} routes wrongly", t.local);
    }
}

#[test]
fn no_upstream_path_is_absolute_or_escapes_the_repo() {
    // Paths are joined onto a raw base URL; a leading slash or a traversal
    // would silently resolve somewhere unintended.
    for t in TRACKED_FOR_TEST.iter() {
        assert!(!t.upstream.starts_with('/'), "{} is absolute", t.upstream);
        assert!(!t.upstream.contains(".."), "{} traverses", t.upstream);
    }
}

#[test]
fn local_names_are_unique() {
    // Two entries writing the same local file would race and the second would
    // overwrite the first's fingerprint.
    let mut names: Vec<&str> = TRACKED_FOR_TEST.iter().map(|t| t.local).collect();
    let before = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(
        before,
        names.len(),
        "duplicate local name in the tracked set"
    );
}
