//! Tests for the `session set-model` selection rules (#465): pair parsing
//! splits on the FIRST slash, target resolution never guesses (ambiguity is
//! an error listing candidates), and `--all` excludes archived sessions.

use crate::cli::session_set_model::{parse_pair, resolve_targets};
use crate::db::models::Session;

fn session(title: &str, archived: bool) -> Session {
    let mut s = Session::new(Some(title.to_string()), None, None);
    if archived {
        s.archived_at = Some(chrono::Utc::now());
    }
    s
}

// ── pair parsing ─────────────────────────────────────────────────────

#[test]
fn pair_splits_on_first_slash_only() {
    let (p, m) = parse_pair("openrouter/tencent/hy3:free").expect("parses");
    assert_eq!(p, "openrouter");
    assert_eq!(m, "tencent/hy3:free");
}

#[test]
fn pair_without_slash_or_empty_halves_is_an_error() {
    assert!(parse_pair("openrouter").is_err());
    assert!(parse_pair("/model-only").is_err());
    assert!(parse_pair("provider/").is_err());
}

// ── target resolution ────────────────────────────────────────────────

#[test]
fn exactly_one_selector_required() {
    let sessions = vec![session("alpha", false)];
    assert!(resolve_targets(&sessions, None, None, false).is_err());
    assert!(resolve_targets(&sessions, Some("ab"), Some("alpha"), false).is_err());
    assert!(resolve_targets(&sessions, Some("ab"), None, true).is_err());
}

#[test]
fn id_prefix_resolves_unique_and_rejects_ambiguous() {
    let a = session("alpha", false);
    let b = session("beta", false);
    let sessions = vec![a.clone(), b.clone()];

    let full = a.id.to_string();
    let hit = resolve_targets(&sessions, Some(&full[..8]), None, false).expect("unique prefix");
    assert_eq!(hit, vec![a.id]);

    // Empty prefix matches everything: ambiguous, candidates listed.
    let err = resolve_targets(&sessions, Some(""), None, false).unwrap_err();
    assert!(err.contains("ambiguous"));
    assert!(err.contains("alpha") && err.contains("beta"));
}

#[test]
fn title_match_is_case_insensitive_and_never_guesses() {
    let sessions = vec![
        session("Telegram: HEY IOLO BUILD", false),
        session("CrabsDev2", false),
        session("crabsland docs", false),
    ];
    let hit = resolve_targets(&sessions, None, Some("hey iolo"), false).expect("unique title");
    assert_eq!(hit.len(), 1);

    let err = resolve_targets(&sessions, None, Some("crabs"), false).unwrap_err();
    assert!(err.contains("matches several"));

    assert!(resolve_targets(&sessions, None, Some("nonexistent"), false).is_err());
}

#[test]
fn all_targets_every_non_archived_session() {
    let sessions = vec![
        session("live-1", false),
        session("live-2", false),
        session("old", true),
    ];
    let hit = resolve_targets(&sessions, None, None, true).expect("all");
    assert_eq!(hit.len(), 2, "archived sessions are never bulk-switched");
}
