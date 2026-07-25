//! Footer text for detached background commands (#762).

use crate::tui::render::background::format_background_tasks;

fn tasks(pairs: &[(&str, u64)]) -> Vec<(String, u64)> {
    pairs.iter().map(|(l, s)| (l.to_string(), *s)).collect()
}

#[test]
fn nothing_running_shows_no_field() {
    // The caller omits the footer slot entirely rather than rendering a blank.
    assert_eq!(format_background_tasks(&[]), None);
}

#[test]
fn one_task_names_it_with_elapsed() {
    assert_eq!(
        format_background_tasks(&tasks(&[("cargo test", 32)])).as_deref(),
        Some("cargo test 32s")
    );
}

#[test]
fn several_tasks_name_the_oldest_and_count_the_rest() {
    // The oldest is the one the user has been waiting on longest, so its
    // elapsed time is the one that answers "is this stuck".
    let t = tasks(&[("cargo test", 90), ("cargo build", 12), ("npm test", 3)]);
    assert_eq!(
        format_background_tasks(&t).as_deref(),
        Some("cargo test 1m 30s +2")
    );
}

#[test]
fn elapsed_humanises_past_a_minute_and_an_hour() {
    assert_eq!(
        format_background_tasks(&tasks(&[("build", 59)])).as_deref(),
        Some("build 59s")
    );
    assert_eq!(
        format_background_tasks(&tasks(&[("build", 60)])).as_deref(),
        Some("build 1m")
    );
    assert_eq!(
        format_background_tasks(&tasks(&[("build", 260)])).as_deref(),
        Some("build 4m 20s")
    );
    assert_eq!(
        format_background_tasks(&tasks(&[("build", 3600)])).as_deref(),
        Some("build 1h")
    );
    assert_eq!(
        format_background_tasks(&tasks(&[("build", 3900)])).as_deref(),
        Some("build 1h 5m")
    );
}

#[test]
fn a_task_that_just_started_reads_as_zero_not_blank() {
    // The field appears the instant the command is spawned; a missing number
    // would look like the indicator itself was broken.
    assert_eq!(
        format_background_tasks(&tasks(&[("cargo test", 0)])).as_deref(),
        Some("cargo test 0s")
    );
}
