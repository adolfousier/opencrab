//! Pure helpers for `opencrabs session set-model` (#465): pair parsing and
//! target resolution, kept side-effect free so the selection rules are unit
//! tested without a database.

use crate::db::models::Session;
use uuid::Uuid;

/// Shared pair parser (#467 uses it on channels and the TUI too).
pub(crate) use crate::utils::provider_pair::parse_pair;

/// Resolve which sessions a set-model invocation targets.
///
/// Exactly one selector must be used: an id prefix, a `--name` title
/// substring, or `--all`. Ambiguous prefixes and titles are an error that
/// lists the candidates, never a guess. `--all` targets every non-archived
/// session.
pub(crate) fn resolve_targets(
    sessions: &[Session],
    id_prefix: Option<&str>,
    name_sub: Option<&str>,
    all: bool,
) -> Result<Vec<Uuid>, String> {
    let selectors =
        usize::from(id_prefix.is_some()) + usize::from(name_sub.is_some()) + usize::from(all);
    if selectors != 1 {
        return Err(
            "pick exactly ONE target: a session id, --name <title match>, or --all".to_string(),
        );
    }

    if all {
        let ids: Vec<Uuid> = sessions
            .iter()
            .filter(|s| s.archived_at.is_none())
            .map(|s| s.id)
            .collect();
        if ids.is_empty() {
            return Err("no non-archived sessions found".to_string());
        }
        return Ok(ids);
    }

    if let Some(prefix) = id_prefix {
        let prefix = prefix.to_lowercase();
        let matches: Vec<&Session> = sessions
            .iter()
            .filter(|s| s.id.to_string().to_lowercase().starts_with(&prefix))
            .collect();
        return match matches.len() {
            0 => Err(format!("no session id starts with '{prefix}'")),
            1 => Ok(vec![matches[0].id]),
            _ => Err(format!(
                "'{prefix}' is ambiguous — candidates:\n{}",
                candidates(&matches)
            )),
        };
    }

    let sub = name_sub.expect("selector count checked").to_lowercase();
    let matches: Vec<&Session> = sessions
        .iter()
        .filter(|s| {
            s.title
                .as_deref()
                .is_some_and(|t| t.to_lowercase().contains(&sub))
        })
        .collect();
    match matches.len() {
        0 => Err(format!("no session title contains '{sub}'")),
        1 => Ok(vec![matches[0].id]),
        _ => Err(format!(
            "'{sub}' matches several sessions — candidates:\n{}",
            candidates(&matches)
        )),
    }
}

pub(crate) fn candidates(matches: &[&Session]) -> String {
    matches
        .iter()
        .map(|s| {
            format!(
                "  {} {}",
                &s.id.to_string()[..8],
                s.title.as_deref().unwrap_or("untitled")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Resolve a user-supplied session id for commands that take exactly one
/// target (#1340): `session list` prints only the first 8 chars of each id,
/// so `session get`/`session notify` must accept what the tool itself shows.
///
/// Full UUIDs parse as a fast path (existing behavior preserved verbatim,
/// including ids not present in the DB, which the caller reports as
/// not-found). Anything else is matched as a case-insensitive prefix with
/// the same rules as [`resolve_targets`]: 0 matches is an error, 1 resolves,
/// several list the candidates.
pub(crate) fn resolve_session_id(sessions: &[Session], id: &str) -> Result<Uuid, String> {
    if let Ok(uuid) = Uuid::parse_str(id) {
        return Ok(uuid);
    }
    let prefix = id.to_lowercase();
    let matches: Vec<&Session> = sessions
        .iter()
        .filter(|s| s.id.to_string().to_lowercase().starts_with(&prefix))
        .collect();
    match matches.len() {
        0 => Err(format!("no session id starts with '{prefix}'")),
        1 => Ok(matches[0].id),
        _ => Err(format!(
            "'{prefix}' is ambiguous — candidates:\n{}",
            candidates(&matches)
        )),
    }
}
