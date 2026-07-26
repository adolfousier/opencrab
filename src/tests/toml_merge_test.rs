//! Additive TOML merge for template sync (#819).
//!
//! Brain files merge by appending markdown sections the local copy lacks.
//! Doing that to TOML appends a duplicate `[table]` and the file stops
//! parsing, which for `usage_pricing.toml` would take the user's pricing
//! config offline entirely.
//!
//! The motivating case is #816/#817: the upstream example gained pricing for
//! two models, users needed those rows, and every rate they had already set
//! had to survive untouched.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::brain::toml_merge::merge_additive;

#[test]
fn a_missing_model_is_added() {
    // The #816/#817 shape: upstream knows a model the local file does not.
    let local = "[providers.qwen]\nglm = 1.0\n";
    let upstream = "[providers.qwen]\nglm = 1.0\nqwen38max = 2.5\n";

    let (merged, report) = merge_additive(local, upstream).unwrap();
    assert!(merged.contains("qwen38max"), "{merged}");
    assert_eq!(report.added, vec!["providers.qwen.qwen38max"]);
}

#[test]
fn a_customised_value_is_never_overwritten() {
    // The load-bearing rule. A local price may be a negotiated rate; upstream
    // knows about new models, not about the user's existing ones.
    let local = "[providers.qwen]\nglm = 9.99\n";
    let upstream = "[providers.qwen]\nglm = 1.0\n";

    let (merged, report) = merge_additive(local, upstream).unwrap();
    assert!(merged.contains("9.99"), "user value must survive: {merged}");
    assert!(!merged.contains("1.0"), "{merged}");
    assert!(report.is_empty());
}

#[test]
fn an_existing_table_is_not_duplicated() {
    // The corruption this exists to prevent: appending the upstream block
    // whole would leave two [providers.qwen] tables and break parsing.
    let local = "[providers.qwen]\nglm = 1.0\n";
    let upstream = "[providers.qwen]\nqwen38max = 2.5\n";

    let (merged, _) = merge_additive(local, upstream).unwrap();
    assert_eq!(
        merged.matches("[providers.qwen]").count(),
        1,
        "duplicate table would fail to parse: {merged}"
    );
    // And it must still parse.
    merged
        .parse::<toml_edit::DocumentMut>()
        .expect("merged output must be valid TOML");
}

#[test]
fn a_whole_missing_table_is_added() {
    let local = "[providers.qwen]\nglm = 1.0\n";
    let upstream = "[providers.qwen]\nglm = 1.0\n\n[providers.anthropic]\nopus5 = 5.0\n";

    let (merged, report) = merge_additive(local, upstream).unwrap();
    assert!(merged.contains("[providers.anthropic]"), "{merged}");
    assert_eq!(report.added, vec!["providers.anthropic"]);
}

#[test]
fn local_comments_and_formatting_survive() {
    // Users annotate these files. A merge that strips their notes is a
    // destructive edit dressed up as an update.
    let local = "# my notes, do not lose these\n[providers.qwen]\nglm = 1.0\n";
    let upstream = "[providers.qwen]\nglm = 1.0\nqwen38max = 2.5\n";

    let (merged, _) = merge_additive(local, upstream).unwrap();
    assert!(merged.contains("# my notes, do not lose these"), "{merged}");
}

#[test]
fn an_unchanged_upstream_changes_nothing() {
    let local = "[providers.qwen]\nglm = 1.0\n";
    let (merged, report) = merge_additive(local, local).unwrap();
    assert!(report.is_empty());
    assert_eq!(merged, local);
}

#[test]
fn a_malformed_upstream_is_refused() {
    // Must not rewrite a working local file from a broken template.
    let local = "[providers.qwen]\nglm = 1.0\n";
    assert!(merge_additive(local, "this is not [ valid toml").is_err());
}

#[test]
fn a_malformed_local_is_refused() {
    // Parsing the local file first means a corrupt one is reported rather
    // than silently replaced by upstream.
    let upstream = "[providers.qwen]\nglm = 1.0\n";
    assert!(merge_additive("not [ valid", upstream).is_err());
}

#[test]
fn nested_additions_reach_inside_existing_tables() {
    // A new model inside a provider the user already has must still arrive.
    let local = "[providers.qwen.entries]\na = 1\n";
    let upstream = "[providers.qwen.entries]\na = 1\nb = 2\n";

    let (merged, report) = merge_additive(local, upstream).unwrap();
    assert!(merged.contains("b = 2"), "{merged}");
    assert_eq!(report.added, vec!["providers.qwen.entries.b"]);
}
