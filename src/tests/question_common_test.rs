//! #764 R1: shared option validation mechanics (`check_options`). The
//! tool-specific error wording is pinned by each tool's own tests; these
//! pin the shared trim/filter/min/max/dedup behavior itself.

use crate::channels::question_common::{OptionsError, check_options};

#[test]
fn trims_and_drops_empties() {
    let out = check_options(
        vec!["  a ".into(), "".into(), "   ".into(), "b".into()],
        1,
        8,
    )
    .expect("two valid options");
    assert_eq!(out, vec!["a", "b"]);
}

#[test]
fn too_few_reports_counts() {
    assert_eq!(
        check_options(vec!["only".into(), "".into()], 2, 8),
        Err(OptionsError::TooFew { got: 1, min: 2 })
    );
}

#[test]
fn too_many_reports_count() {
    let raw: Vec<String> = (0..9).map(|i| i.to_string()).collect();
    assert_eq!(check_options(raw, 1, 8), Err(OptionsError::TooMany(9)));
}

#[test]
fn duplicate_rejected() {
    assert_eq!(
        check_options(vec!["x".into(), " x".into()], 1, 8),
        Err(OptionsError::Duplicate("x".into()))
    );
}

#[test]
fn distinct_options_pass() {
    assert_eq!(
        check_options(vec!["x".into(), "y".into()], 2, 8).expect("distinct pair"),
        vec!["x", "y"]
    );
}
