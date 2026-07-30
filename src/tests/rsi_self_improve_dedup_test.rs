//! RSI re-proposes the SAME improvements every cycle (a tool's failure rate
//! doesn't drop just because the guideline was already written). Before the
//! fix, `self_improve`'s `apply` blindly appended the content each time, so
//! brain files grew with 12–14 duplicate blocks until the dedup-scan cleaned
//! them up — an endless append→dedup→append loop (observed 2026-06-08).
//!
//! The fix routes `apply` through `filter_duplicate_append` (the same dedup
//! `write_opencrabs_file` already uses): a fully-duplicate apply is skipped,
//! and a partially-new apply appends only the genuinely-new paragraphs. These
//! tests exercise the real tool against a throwaway profile home so the
//! integration (apply → dedup → file state) is pinned, not just the helper.

use crate::brain::tools::self_improve::SelfImproveTool;
use crate::brain::tools::{Tool, ToolExecutionContext};
use crate::config::profile::{home_for_profile, with_profile_home_async};
use serde_json::json;
use uuid::Uuid;

const GUIDELINE: &str = "## Bash retry guideline\n\nRetry transient bash failures up to 3 times with backoff before surfacing the error.";
const MARKER: &str = "Retry transient bash failures up to 3 times with backoff";

async fn apply(content: &str) -> crate::brain::tools::ToolResult {
    SelfImproveTool
        .execute(
            json!({
                "action": "apply",
                "target_file": "AGENTS.md",
                "description": "Add retry logic to bash tool",
                "rationale": "Frequent transient failures observed",
                "content": content,
            }),
            &ToolExecutionContext::new(Uuid::new_v4()),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn duplicate_apply_does_not_double_append_to_brain_file() {
    let profile = "rsi-dedup-itest-dup";
    let home = home_for_profile(Some(profile));
    let _ = std::fs::remove_dir_all(&home); // clean slate
    // Seed a realistic AGENTS.md + belief base so the Orient gate (#881)
    // verifies the append against valid anchors (real self_improve appends
    // into a populated brain file, never an empty one).
    std::fs::create_dir_all(&home).unwrap();
    crate::config::profile::seed_brain_templates(&home);

    with_profile_home_async(Some(profile), async {
        // First apply writes the guideline.
        let r1 = apply(GUIDELINE).await;
        assert!(r1.success, "first apply should succeed: {:?}", r1.error);

        // Second, identical apply (what RSI does every cycle) must be SKIPPED,
        // not appended again.
        let r2 = apply(GUIDELINE).await;
        assert!(
            r2.success,
            "duplicate apply should succeed as a skip, not error: {:?}",
            r2.error
        );
        assert!(
            r2.output.to_lowercase().contains("already present")
                || r2.output.to_lowercase().contains("skipped"),
            "duplicate apply must report a skip, got: {}",
            r2.output
        );
    })
    .await;

    let agents = std::fs::read_to_string(home.join("AGENTS.md")).unwrap_or_default();
    let occurrences = agents.matches(MARKER).count();
    let _ = std::fs::remove_dir_all(&home); // cleanup
    assert_eq!(
        occurrences, 1,
        "the guideline must appear exactly once after two identical applies; file:\n{agents}"
    );
}

#[tokio::test]
async fn partially_new_apply_appends_only_the_new_paragraph() {
    let profile = "rsi-dedup-itest-partial";
    let home = home_for_profile(Some(profile));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    crate::config::profile::seed_brain_templates(&home);

    let fresh_marker = "Prefer ripgrep over grep for repo-wide scans";
    let combined = format!("{GUIDELINE}\n\n## Search guideline\n\n{fresh_marker}.");

    with_profile_home_async(Some(profile), async {
        let r1 = apply(GUIDELINE).await;
        assert!(r1.success, "first apply should succeed: {:?}", r1.error);

        // Re-apply the existing guideline PLUS one new paragraph — only the new
        // paragraph should land.
        let r2 = apply(&combined).await;
        assert!(r2.success, "mixed apply should succeed: {:?}", r2.error);
    })
    .await;

    let agents = std::fs::read_to_string(home.join("AGENTS.md")).unwrap_or_default();
    let dup_count = agents.matches(MARKER).count();
    let new_count = agents.matches(fresh_marker).count();
    let _ = std::fs::remove_dir_all(&home);
    assert_eq!(
        dup_count, 1,
        "the already-present guideline must not be duplicated; file:\n{agents}"
    );
    assert_eq!(
        new_count, 1,
        "the genuinely-new paragraph must be appended exactly once; file:\n{agents}"
    );
}
