//! Tests for the `follow_up_question` tool: callback invocation,
//! validation (empty options, oversize, duplicates), and graceful
//! degradation to plain text when no interactive callback is present.

use crate::brain::agent::{FollowUpQuestionInfo, QuestionCallback};
use crate::brain::tools::follow_up_question::{
    FollowUpQuestionTool, MAX_OPTIONS, render_plaintext_question,
};
use crate::brain::tools::{Tool, ToolExecutionContext};
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

fn callback_returning(answer: &'static str) -> QuestionCallback {
    Arc::new(move |_info: FollowUpQuestionInfo| Box::pin(async move { Ok(answer.to_string()) }))
}

fn callback_recording(counter: Arc<AtomicUsize>, answer: &'static str) -> QuestionCallback {
    Arc::new(move |_info: FollowUpQuestionInfo| {
        let counter = counter.clone();
        Box::pin(async move {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(answer.to_string())
        })
    })
}

#[tokio::test]
async fn returns_user_choice() {
    let mut ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    ctx.question_callback = Some(callback_returning("blue"));

    let result = FollowUpQuestionTool
        .execute(
            json!({
                "question": "Pick a color",
                "options": ["red", "blue", "green"]
            }),
            &ctx,
        )
        .await
        .expect("execute");

    assert!(result.success, "error: {:?}", result.error);
    assert!(result.output.contains("blue"));
}

#[tokio::test]
async fn invokes_callback_exactly_once() {
    let counter = Arc::new(AtomicUsize::new(0));
    let mut ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    ctx.question_callback = Some(callback_recording(counter.clone(), "yes"));

    FollowUpQuestionTool
        .execute(
            json!({
                "question": "Continue?",
                "options": ["yes", "no"]
            }),
            &ctx,
        )
        .await
        .expect("execute");

    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn degrades_without_question_callback() {
    let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    // No callback installed (cron/webhook/A2A surface).

    let result = FollowUpQuestionTool
        .execute(
            json!({
                "question": "Pick one",
                "options": ["a", "b"]
            }),
            &ctx,
        )
        .await
        .expect("execute");

    // #716: instead of a hard error, the tool succeeds and hands back the
    // question as plain text for the agent to relay in its reply.
    assert!(
        result.success,
        "should degrade, not error: {:?}",
        result.error
    );
    assert!(result.output.contains("Pick one"));
    assert!(result.output.contains("1. a"));
    assert!(result.output.contains("2. b"));
    assert!(result.output.to_lowercase().contains("plain text"));
}

#[test]
fn plaintext_render_numbers_options() {
    let out = render_plaintext_question("Which environment?", &["dev".into(), "prod".into()]);
    assert!(out.contains("Which environment?"));
    assert!(out.contains("1. dev"));
    assert!(out.contains("2. prod"));
    assert!(!out.ends_with('\n'), "trailing newline should be trimmed");
}

#[tokio::test]
async fn rejects_empty_question() {
    let mut ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    ctx.question_callback = Some(callback_returning("anything"));

    let result = FollowUpQuestionTool
        .execute(
            json!({
                "question": "   ",
                "options": ["a", "b"]
            }),
            &ctx,
        )
        .await
        .expect("execute");

    assert!(!result.success);
    assert!(
        result
            .error
            .unwrap_or_default()
            .contains("non-empty question")
    );
}

#[tokio::test]
async fn rejects_fewer_than_two_options() {
    let mut ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    ctx.question_callback = Some(callback_returning("anything"));

    let result = FollowUpQuestionTool
        .execute(json!({ "question": "?", "options": ["only one"] }), &ctx)
        .await
        .expect("execute");

    assert!(!result.success);
    assert!(
        result
            .error
            .unwrap_or_default()
            .contains("at least 2 non-empty options")
    );
}

#[tokio::test]
async fn drops_blank_options_then_validates_count() {
    let mut ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    ctx.question_callback = Some(callback_returning("anything"));

    // After trimming, only one non-empty option remains -> reject.
    let result = FollowUpQuestionTool
        .execute(
            json!({ "question": "?", "options": ["only", "  ", ""] }),
            &ctx,
        )
        .await
        .expect("execute");

    assert!(!result.success);
}

#[tokio::test]
async fn rejects_too_many_options() {
    let mut ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    ctx.question_callback = Some(callback_returning("anything"));

    let options: Vec<String> = (0..=MAX_OPTIONS).map(|i| format!("opt{i}")).collect();
    let result = FollowUpQuestionTool
        .execute(json!({ "question": "?", "options": options }), &ctx)
        .await
        .expect("execute");

    assert!(!result.success);
    assert!(
        result
            .error
            .unwrap_or_default()
            .contains("Too many options")
    );
}

#[tokio::test]
async fn rejects_duplicate_options() {
    let mut ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    ctx.question_callback = Some(callback_returning("anything"));

    let result = FollowUpQuestionTool
        .execute(
            json!({ "question": "?", "options": ["one", "two", "one"] }),
            &ctx,
        )
        .await
        .expect("execute");

    assert!(!result.success);
    assert!(
        result
            .error
            .unwrap_or_default()
            .contains("Duplicate option")
    );
}

#[test]
fn tool_metadata_is_sane() {
    let tool = FollowUpQuestionTool;
    assert_eq!(tool.name(), "follow_up_question");
    assert!(
        !tool.requires_approval(),
        "the tool IS the user-interaction surface"
    );
    assert!(
        tool.capabilities().is_empty(),
        "no filesystem/shell/network capability"
    );

    let schema = tool.input_schema();
    let props = schema
        .get("properties")
        .and_then(|p| p.as_object())
        .expect("schema has properties");
    assert!(props.contains_key("question"));
    assert!(props.contains_key("options"));
}

/// Options longer than 40 chars are accepted (hard cap was removed).
/// This is a regression test for issue #255.
#[tokio::test]
async fn long_options_pass_through() {
    let mut ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    ctx.question_callback = Some(callback_returning("long option"));

    let long_option =
        "A deliberately long option that exceeds forty characters and should still work";
    assert!(
        long_option.len() > 40,
        "test fixture must be >40 chars, got {}",
        long_option.len()
    );

    let result = FollowUpQuestionTool
        .execute(
            json!({
                "question": "Pick one",
                "options": ["short", long_option]
            }),
            &ctx,
        )
        .await
        .expect("execute");

    assert!(
        result.success,
        "long option should be accepted: {:?}",
        result.error
    );
    assert!(result.output.contains("long option"));
}

// ---------------------------------------------------------------------------
// #1143: compact_options — fold over-long labels into the question body.
// ---------------------------------------------------------------------------

fn info_with(question: &str, options: &[&str]) -> FollowUpQuestionInfo {
    FollowUpQuestionInfo {
        session_id: uuid::Uuid::new_v4(),
        question: question.to_string(),
        options: options.iter().map(|s| s.to_string()).collect(),
    }
}

/// Any label over the threshold folds the full texts into the question
/// body and replaces button labels with numbers.
#[test]
fn compact_options_folds_long_labels_into_question() {
    let long = "production-us-east-1 with full replication and hot standby";
    assert!(long.chars().count() > 40, "fixture must exceed 40");

    let compacted = info_with("Which server?", &["dev-local", long, "staging"]).compact_options(40);

    // Button labels become numbers.
    assert_eq!(compacted.options, vec!["1", "2", "3"]);
    // The question keeps its head and gains a numbered list of the
    // FULL texts — the user still sees exactly what each number means.
    assert!(compacted.question.starts_with("Which server?"));
    assert!(compacted.question.contains("1. dev-local"));
    assert!(compacted.question.contains(&format!("2. {}", long)));
    assert!(compacted.question.contains("3. staging"));
}

/// All labels within the threshold: byte-identical passthrough.
#[test]
fn compact_options_identity_when_labels_fit() {
    let original = info_with("Pick a color", &["red", "blue", "green"]);

    let compacted = original.clone().compact_options(40);

    assert_eq!(compacted.question, "Pick a color");
    assert_eq!(compacted.options, original.options);
}

/// Boundary: exactly at the threshold is fine; one char over folds.
#[test]
fn compact_options_boundary_is_exclusive() {
    let exact: String = "x".repeat(40);
    let over: String = "x".repeat(41);

    let at_threshold = info_with("Q", &["a", exact.as_str()]).compact_options(40);
    assert_eq!(
        at_threshold.options,
        vec!["a", exact.as_str()],
        "40 chars must not fold"
    );

    let over_threshold = info_with("Q", &["a", over.as_str()]).compact_options(40);
    assert_eq!(over_threshold.options, vec!["1", "2"], "41 chars must fold");
}

/// The renderer contract from #1143: the ORIGINAL options (cloned before
/// compaction) are what get registered in the pending-question map, so
/// `options[idx]` resolution delivers the full text to the model. This
/// test pins the clone-before-compact usage pattern the channel
/// renderers follow.
#[test]
fn compact_options_answer_map_keeps_full_texts() {
    let long = "production-us-east-1 with full replication and hot standby";
    let info = info_with("Which server?", &["dev-local", long]);

    // Renderer pattern: clone FIRST, compact for display only.
    let answer_map = info.options.clone();
    let display = info.compact_options(40);

    // A tap on button "2" (callback index 1) resolves via the answer map.
    let model_receives = answer_map
        .get(1)
        .expect("answer map holds every original option");
    assert_eq!(model_receives, long);
    // And the display labels never leak the bare number into an answer.
    assert!(
        display
            .options
            .iter()
            .all(|o| o.chars().all(|c| c.is_ascii_digit()))
    );
}
