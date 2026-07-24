//! #754: the claude-cli model picker must format every row consistently.
//!
//! Before this, `prettify_claude_cli_model` only understood two-part versions
//! (`opus-4-8`), so a major-only release (`opus-5`) fell through to its raw id
//! and sat un-formatted directly below "Opus 4.8". Separately, the bare alias
//! `fable` and the version `fable-5` both rendered "Fable 5", putting the same
//! label on two different rows.

use crate::tui::provider_selector::model_display_label;

#[test]
fn major_only_versions_are_formatted() {
    assert_eq!(model_display_label("opus-5"), "Opus 5");
    assert_eq!(model_display_label("sonnet-5"), "Sonnet 5");
    assert_eq!(model_display_label("fable-5"), "Fable 5");
}

#[test]
fn major_minor_versions_still_formatted() {
    assert_eq!(model_display_label("opus-4-8"), "Opus 4.8");
    assert_eq!(model_display_label("opus-4-7"), "Opus 4.7");
    assert_eq!(model_display_label("sonnet-4-6"), "Sonnet 4.6");
    assert_eq!(model_display_label("haiku-4-5"), "Haiku 4.5");
}

#[test]
fn bare_aliases_are_labelled_as_the_moving_latest_pointer() {
    // They point at whatever is newest, so they must not carry a fixed version.
    assert_eq!(model_display_label("opus"), "Opus (latest)");
    assert_eq!(model_display_label("fable"), "Fable (latest)");
    assert_eq!(model_display_label("sonnet"), "Sonnet (latest)");
    assert_eq!(model_display_label("haiku"), "Haiku (latest)");
}

#[test]
fn no_two_claude_models_share_a_label() {
    // The picker lists these together; duplicate labels are unreadable.
    let ids = crate::brain::provider::claude_cli::available_models();
    let mut labels: Vec<&str> = ids.iter().map(|m| model_display_label(m)).collect();
    labels.sort_unstable();
    let before = labels.len();
    labels.dedup();
    assert_eq!(
        before,
        labels.len(),
        "duplicate labels in the picker: {ids:?}"
    );
}

#[test]
fn every_claude_model_renders_formatted_not_raw() {
    // No row may show a raw lower-case id while its neighbours are formatted.
    for id in crate::brain::provider::claude_cli::available_models() {
        let label = model_display_label(&id);
        assert!(
            label.starts_with(|c: char| c.is_ascii_uppercase()),
            "model {id:?} renders un-formatted as {label:?}"
        );
    }
}

#[test]
fn nonsense_versions_are_left_alone() {
    assert_eq!(model_display_label("opus-x"), "opus-x");
    assert_eq!(model_display_label("opus-"), "opus-");
}
