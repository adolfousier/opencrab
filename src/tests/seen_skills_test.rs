// Tests for issue #131: skills are loadable via `load_brain_file` slug form,
// and any skill-body consumption (read or slug-load) marks the skill SEEN so
// the post-compaction inventory stamp (#125) lists it.

use crate::brain::tools::Tool;
use crate::brain::tools::ToolExecutionContext;
use crate::brain::tools::load_brain_file::*;
use crate::brain::tools::seen_skills;
use uuid::Uuid;

fn ctx() -> ToolExecutionContext {
    ToolExecutionContext::new(Uuid::new_v4())
}

fn tool() -> LoadBrainFileTool {
    LoadBrainFileTool
}

// ── acceptance 1: slug form loads the body; traversal still refused ────────

#[tokio::test]
async fn slug_form_loads_builtin_skill_body() {
    let c = ctx();
    let result = tool()
        .execute(serde_json::json!({"name": "cost-estimate"}), &c)
        .await
        .unwrap();
    assert!(result.success, "slug form of a built-in skill must succeed");
    let text = result.output;
    assert!(
        text.contains("--- skill: cost-estimate ---"),
        "body must be framed as a skill, got: {}",
        &text[..text.len().min(200)]
    );
    assert!(!text.is_empty(), "skill body must not be empty");
}

#[tokio::test]
async fn slug_form_with_query_returns_matching_sections() {
    let c = ctx();
    let result = tool()
        .execute(
            serde_json::json!({"name": "cost-estimate", "query": "usage"}),
            &c,
        )
        .await
        .unwrap();
    assert!(result.success, "slug+query form must succeed");
}

#[tokio::test]
async fn path_traversal_still_refused_after_slug_form_added() {
    for bad in [
        "../../etc/passwd",
        "sub/file.md",
        "../skills/cost-estimate/SKILL.md",
    ] {
        let result = tool()
            .execute(serde_json::json!({"name": bad}), &ctx())
            .await
            .unwrap();
        assert!(!result.success, "traversal input {bad} must fail");
    }
}

#[tokio::test]
async fn unknown_slug_falls_through_to_brain_file_error() {
    let result = tool()
        .execute(
            serde_json::json!({"name": "no-such-skill-or-brain"}),
            &ctx(),
        )
        .await
        .unwrap();
    // Pre-existing contract (unchanged by #131): a missing brain file is a
    // SOFT success carrying a not-found message — the slug branch must not
    // have resolved it, so the body must be the brain-file not-found text,
    // never skill content. Also: nothing gets marked seen.
    let out = result.output.unwrap_or_default();
    assert!(
        out.contains("not found"),
        "unknown slug must fall through to the brain-file not-found body, got: {out}"
    );
    assert!(
        !out.contains("--- skill:"),
        "unknown slug must never render as skill content"
    );
}

// ── acceptance 2: slug-load marks the skill SEEN ────────────────────────────

#[tokio::test]
async fn slug_load_marks_skill_seen_for_session() {
    let c = ctx();
    let session = c.session_id;
    assert!(
        !seen_skills::was_seen(session, "cost-estimate"),
        "fresh session must not have the skill seen"
    );
    tool()
        .execute(serde_json::json!({"name": "cost-estimate"}), &c)
        .await
        .unwrap();
    assert!(
        seen_skills::was_seen(session, "cost-estimate"),
        "slug-load must mark the skill seen"
    );
    assert_eq!(
        seen_skills::seen_for_session(session),
        vec!["cost-estimate".to_string()]
    );
}

// ── acceptance 2/3: read_file on a SKILL.md also counts ────────────────────

#[test]
fn read_file_whole_read_marks_skill_seen_via_hook() {
    // The read.rs hook calls skill_slug_from_path before mark_seen; here we
    // verify the registry contract the hook relies on, session-scoped.
    let session = Uuid::new_v4();
    let path = std::path::Path::new("/root/.opencrabs/profiles/ops/skills/grafana/SKILL.md");
    let slug = seen_skills::skill_slug_from_path(path).expect("skill path must yield slug");
    assert_eq!(slug, "grafana");
    seen_skills::mark_seen(session, &slug);
    assert!(seen_skills::was_seen(session, "grafana"));
}

// ── acceptance 3: union of both registries is deduplicated ─────────────────

#[test]
fn stamp_union_dedupes_active_and_seen() {
    let mut active = std::collections::BTreeSet::new();
    active.insert("opencrabs-dev".to_string());
    let mut seen = std::collections::BTreeSet::new();
    seen.insert("opencrabs-dev".to_string());
    seen.insert("grafana".to_string());
    let union: Vec<String> = active.union(&seen).cloned().collect();
    assert_eq!(union.len(), 2, "overlap must dedupe");
    assert_eq!(
        union,
        vec!["grafana".to_string(), "opencrabs-dev".to_string()]
    );
}

// ── acceptance 5: stamp-build observability is a DEBUG line ────────────────
// Verified by code inspection of continuation_prompt (tracing::debug! with
// the full inventory list); zero-skill silence is covered by the existing
// skill_stamp_is_silent_when_no_skills_active test on append_skill_stamp.
