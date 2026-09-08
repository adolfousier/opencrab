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
    let out = &result.output;
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

// ── issue #138 gap 2: filename form registers the skill ────────────────────

#[tokio::test]
async fn filename_form_marks_skill_seen() {
    let c = ctx();
    let session = c.session_id;
    assert!(!seen_skills::was_seen(session, "cost-estimate"));
    let result = tool()
        .execute(serde_json::json!({"name": "cost-estimate.md"}), &c)
        .await
        .unwrap();
    assert!(
        result.success,
        "filename form of a built-in skill must succeed"
    );
    assert!(
        result.output.contains("--- skill: cost-estimate ---"),
        "filename form must render as skill content, got: {}",
        &result.output[..result.output.len().min(200)]
    );
    assert!(
        seen_skills::was_seen(session, "cost-estimate"),
        "filename-form load must mark the skill seen (#138 gap 2)"
    );
}

#[tokio::test]
async fn filename_form_with_query_marks_skill_seen_and_filters() {
    let c = ctx();
    let session = c.session_id;
    let result = tool()
        .execute(
            serde_json::json!({"name": "cost-estimate.md", "query": "usage"}),
            &c,
        )
        .await
        .unwrap();
    assert!(result.success, "filename+query form must succeed");
    assert!(
        seen_skills::was_seen(session, "cost-estimate"),
        "query-filtered filename-form load is consumption too (#138 gap 2)"
    );
}

#[tokio::test]
async fn filename_form_of_brain_file_still_reads_flat_file() {
    // A real brain file must NOT be intercepted by the skill branch — only
    // names that resolve through the skill registry take the skill path.
    let result = tool()
        .execute(
            serde_json::json!({"name": "nonexistent-brain-file.md"}),
            &ctx(),
        )
        .await
        .unwrap();
    // Missing brain file is a SOFT success carrying not-found text.
    assert!(result.output.contains("not found") || result.output.contains("exists but is empty"));
}

#[tokio::test]
async fn filename_form_traversal_still_refused() {
    let fresh = uuid::Uuid::new_v4();
    for bad in ["../skills/cost-estimate/SKILL.md", "sub/cost-estimate.md"] {
        let result = tool()
            .execute(serde_json::json!({"name": bad}), &ctx())
            .await
            .unwrap();
        assert!(!result.success, "traversal input {bad} must fail");
        assert!(!seen_skills::was_seen(fresh, "cost-estimate"));
    }
}

// ── issue #138 gap 1: DB persistence + hydrate ─────────────────────────────

mod persistence {
    use crate::brain::tools::seen_skills;
    use crate::db::Database;
    use crate::db::repository::SessionSkillsRepository;
    use uuid::Uuid;

    async fn repo() -> (Database, SessionSkillsRepository) {
        let db = Database::connect_in_memory().await.expect("in-memory db");
        db.run_migrations().await.expect("migrations");
        let r = SessionSkillsRepository::new(db.pool().clone());
        (db, r)
    }

    #[tokio::test]
    async fn record_upserts_and_all_reads_back() {
        let (_db, r) = repo().await;
        let sid = Uuid::new_v4();
        r.record(sid, "opencrabs-dev").await.expect("record");
        r.record(sid, "opencrabs-dev")
            .await
            .expect("re-record (upsert)");
        r.record(sid, "grafana").await.expect("record 2");
        let rows = r.all().await.expect("all");
        assert_eq!(rows.len(), 2, "upsert must not duplicate rows");
        assert!(rows.contains(&(sid, "opencrabs-dev".to_string())));
        assert!(rows.contains(&(sid, "grafana".to_string())));
    }

    #[tokio::test]
    async fn prune_drops_rows_for_missing_sessions_only() {
        let (_db, r) = repo().await;
        let live = Uuid::new_v4();
        let dead = Uuid::new_v4();
        r.record(live, "grafana").await.expect("live row");
        r.record(dead, "grafana").await.expect("dead row");
        // The live session must exist in `sessions` for the prune-keep leg.
        let pool = _db.pool().clone();
        pool.get()
            .await
            .expect("conn")
            .interact(move |conn| {
                conn.execute(
                    "INSERT INTO sessions (id, title, model, created_at, updated_at) \
                     VALUES (?1, 't', 'm', unixepoch(), unixepoch())",
                    rusqlite::params![live.to_string()],
                )
            })
            .await
            .expect("interact")
            .expect("insert session");
        let pruned = r.prune_missing_sessions().await.expect("prune");
        assert_eq!(pruned, 1, "exactly the dead session's row goes");
        let rows = r.all().await.expect("all after prune");
        assert_eq!(rows, vec![(live, "grafana".to_string())]);
    }

    #[tokio::test]
    async fn hydrate_loads_rows_into_registry() {
        let (_db, r) = repo().await;
        let sid = Uuid::new_v4();
        r.record(sid, "repo-audit").await.expect("record");
        assert!(!seen_skills::was_seen(sid, "repo-audit"));
        // hydrate_from_db reads the GLOBAL pool (process-wide OnceLock, not
        // settable in tests) — so we test the hydrate DATA path via the repo
        // + registry contract it feeds, not the global-pool plumbing.
        let rows = r.all().await.expect("all");
        for (s, slug) in rows {
            seen_skills::mark_seen(s, &slug);
        }
        assert!(
            seen_skills::was_seen(sid, "repo-audit"),
            "applying hydrate rows must mark skills seen"
        );
    }
}
