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

// ── #1176 G1/G3: per-channel budgets + the single char-based truncator ──────

#[test]
fn truncate_label_passthrough_under_budget() {
    let out = crate::channels::question_common::truncate_label("short", 10);
    assert_eq!(out, "short");
}

#[test]
fn truncate_label_exact_budget_is_passthrough() {
    let s = "exact!";
    let out = crate::channels::question_common::truncate_label(s, s.chars().count());
    assert_eq!(out, s);
}

#[test]
fn truncate_label_cuts_on_char_boundaries_with_ellipsis() {
    // multibyte probe: budget 5 -> cut 2 CHARS + "..." — never splits a glyph
    // (the byte-based truncate_str this replaces would have panicked here).
    let out = crate::channels::question_common::truncate_label("héllo🎉world", 5);
    assert!(out.starts_with("hé"));
    assert!(out.ends_with("..."));
    assert!(out.chars().count() <= 5);
}

#[test]
fn telegram_budget_reproduces_the_historic_fold_shape() {
    // over-budget label -> (budget-3) chars + "...", exactly the old
    // take(57)+"..." contract at budget 60.
    let long = "x".repeat(100);
    let out = crate::channels::question_common::truncate_label(
        &long,
        crate::channels::question_common::TELEGRAM_LABEL_BUDGET,
    );
    assert_eq!(out.chars().count(), 60);
    assert!(out.ends_with("..."));
}

#[test]
fn channel_budgets_stay_sane_and_fold_aware() {
    use crate::channels::question_common::{
        DISCORD_LABEL_BUDGET, FOLD_THRESHOLD, SLACK_LABEL_BUDGET, TELEGRAM_LABEL_BUDGET,
    };
    assert!(FOLD_THRESHOLD > 0);
    assert!(
        TELEGRAM_LABEL_BUDGET > FOLD_THRESHOLD,
        "fold must fire before any single label gets truncated"
    );
    assert!(DISCORD_LABEL_BUDGET > 0 && SLACK_LABEL_BUDGET > 0);
}
