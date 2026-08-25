//! Tests for the `suggest_options` tool: option validation via
//! `sanitize_options`, and the non-blocking execute path (fires a progress
//! event through the callback and always succeeds; no callback is a no-op, not
//! an error, because the suggestions are optional).

use crate::brain::agent::ProgressEvent;
use crate::brain::tools::suggest_options::{MAX_OPTIONS, SuggestOptionsTool, sanitize_options};
use crate::brain::tools::{Tool, ToolExecutionContext};
use serde_json::json;
use std::sync::Arc;
use std::sync::Mutex;

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn accepts_one_to_max_distinct() {
    assert_eq!(sanitize_options(strs(&["do the thing"])).unwrap().len(), 1);
    // Contract-relative fixture: exactly MAX_OPTIONS distinct options are accepted.
    let max: Vec<String> = (1..=MAX_OPTIONS).map(|i| format!("option {i}")).collect();
    assert_eq!(sanitize_options(max).unwrap().len(), MAX_OPTIONS);
}

#[test]
fn trims_and_drops_empties() {
    let out = sanitize_options(strs(&["  keep  ", "   ", "also"])).unwrap();
    assert_eq!(out, vec!["keep".to_string(), "also".to_string()]);
}

#[test]
fn rejects_empty_after_trim() {
    assert!(sanitize_options(strs(&["   ", ""])).is_err());
    assert!(sanitize_options(vec![]).is_err());
}

#[test]
fn rejects_over_cap() {
    // Contract-relative fixture: MAX_OPTIONS + 1 must be rejected.
    let over: Vec<String> = (0..=MAX_OPTIONS).map(|i| format!("option {i}")).collect();
    assert_eq!(over.len(), MAX_OPTIONS + 1);
    let err = sanitize_options(over).unwrap_err();
    assert!(err.contains("Too many"), "got: {err}");
}

#[test]
fn rejects_duplicates() {
    let err = sanitize_options(strs(&["same", "same"])).unwrap_err();
    assert!(err.contains("Duplicate"), "got: {err}");
}

#[tokio::test]
async fn execute_fires_progress_event() {
    let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = captured.clone();
    let cb: crate::brain::agent::ProgressCallback = Arc::new(move |_sid, event| {
        if let ProgressEvent::SuggestedOptions(opts) = event {
            *sink.lock().unwrap() = opts;
        }
    });
    let mut ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    ctx.progress_callback = Some(cb);

    let result = SuggestOptionsTool
        .execute(
            json!({ "options": ["run the tests", "show the diff"] }),
            &ctx,
        )
        .await
        .expect("execute");

    assert!(result.success, "error: {:?}", result.error);
    assert_eq!(
        *captured.lock().unwrap(),
        vec!["run the tests".to_string(), "show the diff".to_string()]
    );
}

#[tokio::test]
async fn execute_without_callback_is_ok_noop() {
    // Surfaces with no progress bridge just don't render — never an error.
    let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    let result = SuggestOptionsTool
        .execute(json!({ "options": ["one thing"] }), &ctx)
        .await
        .expect("execute");
    assert!(result.success);
}

#[tokio::test]
async fn execute_rejects_bad_options() {
    let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    let result = SuggestOptionsTool
        .execute(json!({ "options": [] }), &ctx)
        .await
        .expect("execute");
    assert!(!result.success);
}

#[test]
fn tool_is_non_blocking_metadata() {
    let tool = SuggestOptionsTool;
    assert_eq!(tool.name(), "suggest_options");
    assert!(!tool.requires_approval());
}

#[test]
fn description_cap_tracks_max_options_const() {
    use crate::brain::tools::Tool;
    use crate::brain::tools::suggest_options::{MAX_OPTIONS, SuggestOptionsTool};

    let d = Tool::description(&SuggestOptionsTool);
    assert!(
        d.contains(&format!("up to {MAX_OPTIONS}")),
        "description cap must track MAX_OPTIONS, not a restated literal (#1176)"
    );
    assert!(
        !d.contains("1-4"),
        "stale pre-merge cap range must not return to the description"
    );
}
