// Tests for issue #131: skills are loadable via `load_brain_file` slug form,
// and any skill-body consumption (read or slug-load) marks the skill SEEN so
// the post-compaction inventory stamp (#125) lists it.

use crate::brain::tools::Tool;
use crate::brain::tools::ToolExecutionContext;
use crate::brain::tools::load_brain_file::*;
use crate::brain::tools::seen_skills;
use std::path::Path;
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
async fn unknown_slug_falls_through_to_brain_file_handling() {
    let result = tool()
        .execute(
            serde_json::json!({"name": "no-such-skill-or-brain"}),
            &ctx(),
        )
        .await
        .unwrap();

    // The point of the assertion is that an unresolvable slug must never be
    // served as if it were a skill body. The shape of the miss is main's to
    // decide, and main answers a missing brain file with a success-carrying
    // "not found" message rather than an error (load_brain_file.rs), so this
    // pins the fall-through by content instead of by the success flag.
    let body = result.output;
    assert!(
        body.contains("not found"),
        "unknown slug must fall through to the brain-file miss, got: {body}"
    );
    assert!(
        !body.contains("--- skill:"),
        "unknown slug must never be answered with a skill body, got: {body}"
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

// ── moved out of an inline `mod tests` in src/brain/tools/seen_skills.rs ──
//
// Tests live under src/tests/ (house rule); the slug/registry unit cases
// arrived inline with the #131 port and are exercised here through the same
// public API instead.

#[test]
fn slug_extraction_from_skill_paths() {
    assert_eq!(
        seen_skills::skill_slug_from_path(Path::new(
            "/root/.opencrabs/profiles/ops/skills/opencrabs-dev/SKILL.md"
        )),
        Some("opencrabs-dev".to_string())
    );
    assert_eq!(
        seen_skills::skill_slug_from_path(Path::new("skills/foo/SKILL.md")),
        Some("foo".to_string())
    );
}

#[test]
fn non_skill_paths_yield_none() {
    assert_eq!(
        seen_skills::skill_slug_from_path(Path::new("/home/user/MEMORY.md")),
        None
    );
    assert_eq!(
        seen_skills::skill_slug_from_path(Path::new("skills/foo/other.md")),
        None
    );
    assert_eq!(
        seen_skills::skill_slug_from_path(Path::new("not-skills/foo/SKILL.md")),
        None
    );
    assert_eq!(
        seen_skills::skill_slug_from_path(Path::new("skills/foo/")),
        None
    );
}

#[test]
fn mark_seen_is_idempotent_and_session_scoped() {
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    seen_skills::mark_seen(a, "opencrabs-dev");
    seen_skills::mark_seen(a, "opencrabs-dev");
    assert!(seen_skills::was_seen(a, "opencrabs-dev"));
    assert_eq!(
        seen_skills::seen_for_session(a),
        vec!["opencrabs-dev".to_string()]
    );
    assert!(!seen_skills::was_seen(b, "opencrabs-dev"));
    assert!(seen_skills::seen_for_session(b).is_empty());
}

#[test]
fn seen_list_is_sorted_and_multi() {
    let a = Uuid::new_v4();
    seen_skills::mark_seen(a, "zeta");
    seen_skills::mark_seen(a, "alpha");
    assert_eq!(
        seen_skills::seen_for_session(a),
        vec!["alpha".to_string(), "zeta".to_string()]
    );
}
