//! Session-scoped seen-skill tracking (issue #131).
//!
//! Records which skill bodies a session has CONSUMED — by any surface — so
//! the post-compaction advisory stamp (#125) can list skills the agent
//! actually read, not only those invoked via slash command.
//!
//! Two hooks feed this registry:
//! - `load_brain_file` with a bare skill slug (the #131 canonical form)
//! - `read_file` on a `skills/<slug>/SKILL.md` path (whole-file reads)
//!
//! This is deliberately SEPARATE from `AgentService::active_skills` (the
//! #219 slash-invocation registry): that set also drives per-turn body
//! re-injection into the system prompt, and read-counted skills must not be
//! re-injected on top of the read already present in conversation history.
//! The compaction stamp is the UNION of both registries.

use std::collections::{BTreeSet, HashSet};
use std::path::Path;
use std::sync::OnceLock;
use uuid::Uuid;

fn registry() -> &'static std::sync::Mutex<HashSet<(Uuid, String)>> {
    static REGISTRY: OnceLock<std::sync::Mutex<HashSet<(Uuid, String)>>> = OnceLock::new();
    REGISTRY.get_or_init(|| std::sync::Mutex::new(HashSet::new()))
}

/// Extract the skill slug from a path that points at a skill definition
/// file: any path whose second-to-last component is `skills` and whose
/// file name is `SKILL.md` yields `Some(slug)`. Returns `None` for
/// everything else (brain files, regular files, skill assets).
pub fn skill_slug_from_path(path: &Path) -> Option<String> {
    let file_name = path.file_name()?.to_str()?;
    if file_name != "SKILL.md" {
        return None;
    }
    let mut comps = path.components().rev();
    comps.next()?; // SKILL.md
    let slug = comps.next()?;
    if comps.next()?.as_os_str() != "skills" {
        return None;
    }
    slug.as_os_str().to_str().map(|s| s.to_string())
}

/// Record that `session_id` consumed skill `slug` (via read or slug-form
/// load). Idempotent per (session, slug).
pub fn mark_seen(session_id: Uuid, slug: &str) {
    registry()
        .lock()
        .expect("seen_skills registry poisoned")
        .insert((session_id, slug.to_string()));
}

/// Whether `session_id` has consumed skill `slug` this run.
pub fn was_seen(session_id: Uuid, slug: &str) -> bool {
    registry()
        .lock()
        .expect("seen_skills registry poisoned")
        .contains(&(session_id, slug.to_string()))
}

/// All skills `session_id` has consumed, sorted (deterministic stamp order).
pub fn seen_for_session(session_id: Uuid) -> Vec<String> {
    let all: BTreeSet<String> = registry()
        .lock()
        .expect("seen_skills registry poisoned")
        .iter()
        .filter(|(s, _)| *s == session_id)
        .map(|(_, slug)| slug.clone())
        .collect();
    all.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_extraction_from_skill_paths() {
        assert_eq!(
            skill_slug_from_path(Path::new(
                "/root/.opencrabs/profiles/ops/skills/opencrabs-dev/SKILL.md"
            )),
            Some("opencrabs-dev".to_string())
        );
        assert_eq!(
            skill_slug_from_path(Path::new("skills/foo/SKILL.md")),
            Some("foo".to_string())
        );
    }

    #[test]
    fn non_skill_paths_yield_none() {
        assert_eq!(
            skill_slug_from_path(Path::new("/home/user/MEMORY.md")),
            None
        );
        assert_eq!(skill_slug_from_path(Path::new("skills/foo/other.md")), None);
        assert_eq!(
            skill_slug_from_path(Path::new("not-skills/foo/SKILL.md")),
            None
        );
        assert_eq!(skill_slug_from_path(Path::new("skills/foo/")), None);
    }

    #[test]
    fn mark_seen_is_idempotent_and_session_scoped() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        mark_seen(a, "opencrabs-dev");
        mark_seen(a, "opencrabs-dev");
        assert!(was_seen(a, "opencrabs-dev"));
        assert_eq!(seen_for_session(a), vec!["opencrabs-dev".to_string()]);
        assert!(!was_seen(b, "opencrabs-dev"));
        assert!(seen_for_session(b).is_empty());
    }

    #[test]
    fn seen_list_is_sorted_and_multi() {
        let a = Uuid::new_v4();
        mark_seen(a, "zeta");
        mark_seen(a, "alpha");
        assert_eq!(
            seen_for_session(a),
            vec!["alpha".to_string(), "zeta".to_string()]
        );
    }
}
