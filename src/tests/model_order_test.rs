//! Newest-first ordering for the model picker (#1057).
//!
//! The list was insertion-ordered and new models are appended, so every new
//! release landed at the bottom. Plain alphabetical does not fix that:
//! ascending text order puts `glm-5.3s` after `glm-4.5` anyway, and
//! `glm-5.10` sorts before `glm-5.2` as text while being newer.

use crate::tui::model_order::sort_newest_first;

fn ordered(input: &[&str]) -> Vec<String> {
    let mut v: Vec<&str> = input.to_vec();
    sort_newest_first(&mut v);
    v.into_iter().map(str::to_string).collect()
}

#[test]
fn the_newest_model_leads_the_list() {
    // The reported config, verbatim: glm-5.3s was last on disk.
    let out = ordered(&[
        "glm-5.2",
        "glm-5.1",
        "glm-5-turbo",
        "glm-5",
        "glm-4.7",
        "glm-4.6",
        "glm-4.5",
        "glm-4.5-air",
        "glm-5.3s",
    ]);
    assert_eq!(out[0], "glm-5.3s", "the newest release must lead");
    assert!(
        out.iter().position(|m| m == "glm-4.5").unwrap()
            > out.iter().position(|m| m == "glm-5").unwrap(),
        "4.x must fall below 5.x"
    );
}

#[test]
fn version_segments_compare_as_numbers_not_text() {
    // The case plain alphabetical gets wrong: 10 > 2 numerically, but "10"
    // sorts before "2" as text.
    let out = ordered(&["glm-5.2", "glm-5.10", "glm-5.9"]);
    assert_eq!(out, vec!["glm-5.10", "glm-5.9", "glm-5.2"]);
}

#[test]
fn a_newly_appended_model_does_not_land_at_the_bottom() {
    // The defect itself: registration appends, so this is the shape that
    // regressed.
    let out = ordered(&["glm-4.5", "glm-4.6", "glm-5.4"]);
    assert_eq!(out[0], "glm-5.4");
}

#[test]
fn ordering_is_case_insensitive_across_display_names_and_ids() {
    // The picker mixes catalogue display names with raw ids, so `GLM 5.1`
    // and `glm-5.1` must not sort as if they were different versions.
    let out = ordered(&["glm-4.7", "GLM 5.1", "glm-5"]);
    assert_eq!(out[0], "GLM 5.1");
    assert_eq!(out[2], "glm-4.7");
}

#[test]
fn a_suffixed_variant_stays_beside_its_base_version() {
    let out = ordered(&["glm-4.5", "glm-4.5-air", "glm-5"]);
    assert_eq!(out[0], "glm-5");
    let air = out.iter().position(|m| m == "glm-4.5-air").unwrap();
    let base = out.iter().position(|m| m == "glm-4.5").unwrap();
    assert_eq!(air.abs_diff(base), 1, "variant sits next to its base");
}

#[test]
fn separators_never_outrank_the_version_number() {
    // A display name and a raw id differ only in separator; comparing those
    // runs as text let ' ' vs '-' decide the order instead of 5 vs 4.
    let out = ordered(&["glm-4.9", "GLM 5.1"]);
    assert_eq!(out[0], "GLM 5.1");
}

#[test]
fn unversioned_names_do_not_panic_or_vanish() {
    let out = ordered(&["coder-model", "glm-5", "opus"]);
    assert_eq!(out.len(), 3);
    assert!(out.contains(&"coder-model".to_string()));
}

#[test]
fn an_absurd_digit_run_saturates_instead_of_panicking() {
    // A wrong order beats a panic inside a picker.
    let long = format!("glm-{}", "9".repeat(40));
    let out = ordered(&[&long, "glm-5"]);
    assert_eq!(out.len(), 2);
}

#[test]
fn ordering_an_empty_or_single_list_is_a_no_op() {
    assert!(ordered(&[]).is_empty());
    assert_eq!(ordered(&["glm-5"]), vec!["glm-5"]);
}
