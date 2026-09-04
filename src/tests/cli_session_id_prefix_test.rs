//! Regression tests for #1340: `session list` displays only the first 8
//! chars of each session id, but `session get` demanded the full UUID —
//! the id the tool itself showed was unusable in its own next command
//! (reported by arfonzo: `session get 5c922776` → "invalid length: found 8").
//!
//! The shared resolver `resolve_session_id` reuses `resolve_targets`
//! semantics: full UUIDs pass through untouched, anything else is a
//! case-insensitive prefix match where 0 is an error, 1 resolves, and
//! several list the candidates.

use crate::cli::session_resolve::resolve_session_id;
use crate::db::models::Session;

/// Two deterministic ids sharing arfonzo's literal 8-char prefix.
const ID_A: &str = "5c922776-0000-4000-8000-000000000001";
const ID_B: &str = "5c922776-0000-4000-8000-000000000002";

fn session(title: &str) -> Session {
    Session::new(Some(title.to_string()), None, None)
}

fn session_with_id(id: &str, title: &str) -> Session {
    let mut s = session(title);
    s.id = uuid::Uuid::parse_str(id).expect("valid test uuid");
    s
}

#[test]
fn full_uuid_passes_through_verbatim() {
    let s = session("alpha");
    let sessions = vec![s.clone()];
    let resolved = resolve_session_id(&sessions, &s.id.to_string()).expect("exact uuid");
    assert_eq!(resolved, s.id);
}

#[test]
fn exact_uuid_not_in_db_still_takes_fast_path() {
    // Deliberate: the fast path parses without requiring membership, so a
    // full UUID the DB does not know still reaches the caller and surfaces
    // the pre-existing "Session not found" message instead of a prefix error.
    let sessions = vec![session("alpha")];
    let stranger = uuid::Uuid::new_v4();
    let resolved = resolve_session_id(&sessions, &stranger.to_string()).expect("parse fast path");
    assert_eq!(resolved, stranger);
}

#[test]
fn eight_char_prefix_resolves_unique_session() {
    let a = session("orchestrator");
    let b = session("worker");
    let sessions = vec![a.clone(), b.clone()];
    let prefix = &a.id.to_string()[..8];
    let resolved = resolve_session_id(&sessions, prefix).expect("unique prefix");
    assert_eq!(resolved, a.id);
    assert_ne!(resolved, b.id);
}

#[test]
fn prefix_match_is_case_insensitive() {
    let a = session_with_id(ID_A, "orchestrator");
    let sessions = vec![a.clone()];
    let upper = a.id.to_string()[..8].to_uppercase();
    let resolved = resolve_session_id(&sessions, &upper).expect("uppercase prefix");
    assert_eq!(resolved, a.id);
}

#[test]
fn arfonzos_exact_case_resolves() {
    // The literal report: `session get 5c922776` after
    // `session list` printed `5c922776 Main Orchestrator`.
    let a = session_with_id(ID_A, "Main Orchestrator");
    let sessions = vec![a.clone()];
    let resolved = resolve_session_id(&sessions, "5c922776").expect("the reported prefix");
    assert_eq!(resolved.to_string(), ID_A);
}

#[test]
fn ambiguous_prefix_lists_candidates() {
    let a = session_with_id(ID_A, "Main Orchestrator");
    let b = session_with_id(ID_B, "worker");
    let sessions = vec![a, b];
    let err = resolve_session_id(&sessions, "5c922776").expect_err("ambiguous prefix");
    assert!(err.contains("ambiguous"), "got: {err}");
    assert!(
        err.contains("Main Orchestrator"),
        "candidates list the title: {err}"
    );
    assert!(err.contains("worker"), "candidates list every match: {err}");
}

#[test]
fn unknown_prefix_errors_cleanly() {
    let sessions = vec![session("alpha")];
    let err = resolve_session_id(&sessions, "zzzzzzzz").expect_err("no match");
    assert!(
        err.contains("no session id starts with 'zzzzzzzz'"),
        "got: {err}"
    );
}

#[test]
fn garbage_input_never_panics() {
    let sessions = vec![session("alpha")];
    let err = resolve_session_id(&sessions, "not-a-uuid-at-all!!").expect_err("garbage input");
    assert!(err.contains("no session id starts with"), "got: {err}");
}

#[test]
fn empty_prefix_is_rejected_not_wildcard() {
    // An empty string prefix would match EVERY session; that must stay an
    // ambiguity error rather than silently resolving the first row.
    let a = session_with_id(ID_A, "alpha");
    let b = session_with_id(ID_B, "beta");
    let sessions = vec![a, b];
    assert!(resolve_session_id(&sessions, "").is_err());
}
