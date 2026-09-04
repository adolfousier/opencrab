//! The set of real config sections, and what a write may address (#1199).
//!
//! Reads and writes disagreed about what a section is. The read path resolves
//! shorthand (`stt` -> `providers`) against a registry; the write path
//! accepted any dotted string and created whatever tables it named. A caller
//! that passed `opencode` instead of `providers.opencode` got `Ok(())`, an
//! orphan `[opencode]` table at the end of `config.toml`, and a value serde
//! discards on every load. Reads then keep reporting the old value, which
//! looks exactly like a stale cache.
//!
//! The registry lives here rather than in the config tool because both sides
//! need it and `config` cannot depend on `brain::tools`.

/// Top-level tables that actually exist in `config.toml`.
///
/// A write path must START at one of these. Anything else is an orphan by
/// construction, whatever it looks like.
pub const CONFIG_SECTIONS: &[&str] = &[
    "agent",
    "voice",
    "logging",
    "debug",
    "channels",
    "provider_registry",
    "database",
    "providers",
    "tui",
];

/// Children whose parent section is not guessable from the name alone.
///
/// Used two ways: the read path accepts them as shorthand, and the write path
/// turns them into the suggestion in its error, since a caller writing
/// `fallback` almost certainly means `providers.fallback`.
pub const SECTION_PARENTS: &[(&str, &str)] = &[
    ("telegram", "channels"),
    ("discord", "channels"),
    ("slack", "channels"),
    ("whatsapp", "channels"),
    ("trello", "channels"),
    ("stt", "providers"),
    ("tts", "providers"),
    ("fallback", "providers"),
    ("custom", "providers"),
];

/// Resolve what a READER asked for to a top-level section (#889).
///
/// Config is nested but the config tool only renders the first level, so the
/// paths people actually write were rejected: every recorded failure was
/// `providers.stt`, `stt` or `telegram`. Accepts an exact section, a dotted
/// path (`providers.stt` -> `providers`), or a known child
/// (`telegram` -> `channels`). `None` when nothing matches, so the caller can
/// refuse rather than guess.
pub fn resolve_section(requested: &str) -> Option<&'static str> {
    let want = requested.trim().trim_matches('.').to_lowercase();
    if want.is_empty() {
        return None;
    }
    let head = want.split('.').next().unwrap_or(&want);
    if let Some(hit) = CONFIG_SECTIONS.iter().find(|s| **s == head) {
        return Some(hit);
    }
    SECTION_PARENTS
        .iter()
        .find(|(child, _)| *child == head)
        .map(|(_, parent)| *parent)
}

/// Can a WRITE address `section`, or would it create an orphan table?
///
/// Deliberately stricter than [`resolve_section`], which exists to accept
/// shorthand. Shorthand is fine to read through and fatal to write through:
/// `custom.inferhub` resolves to `providers` for a reader, but writing it
/// creates a top-level `[custom.inferhub]` that serde ignores. So the rule is
/// positional rather than by-name — the FIRST segment must itself be a real
/// top-level table.
///
/// Returns `Ok(())` or a message naming the likely intent and the valid roots.
pub fn validate_write_path(section: &str) -> Result<(), String> {
    let trimmed = section.trim().trim_matches('.');
    if trimmed.is_empty() {
        return Err("config section is empty".to_string());
    }
    let head = trimmed.split('.').next().unwrap_or(trimmed).to_lowercase();
    if CONFIG_SECTIONS.contains(&head.as_str()) {
        return Ok(());
    }

    // A known child written as if it were top-level: the single most likely
    // mistake, and the one actually observed. Name the fix rather than just
    // the rule.
    let suggestion = SECTION_PARENTS
        .iter()
        .find(|(child, _)| *child == head)
        .map(|(_, parent)| format!(" — did you mean '{parent}.{trimmed}'?"))
        .unwrap_or_default();

    Err(format!(
        "unknown config section '{trimmed}'{suggestion} Writes must start at a real \
         top-level section: {}. Writing anything else creates a table serde ignores on \
         load, so the value would silently never apply.",
        CONFIG_SECTIONS.join(", ")
    ))
}
