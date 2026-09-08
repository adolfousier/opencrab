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

use crate::config::Config;
/// Top-level tables that actually exist in `config.toml`.
///
/// A write path must START at one of these. Anything else is an orphan by
/// construction, whatever it looks like.
///
/// Pinned to the compiled schema by the drift guard in
/// `src/tests/rsi_stale_scan_test.rs` (#1385): this list once carried a
/// phantom `voice` entry and lacked eight real sections (`daemon`, `a2a`,
/// `image`, `cron`, `memory`, `brain`, `browser`, `doctor`), so writes to
/// real sections were refused while writes to `voice` created tables serde
/// silently discarded.
///
/// `voice` is deliberately absent: it is a derived read-only view over
/// `providers.stt`/`providers.tts`, not a table. `config read voice` still
/// works (dispatched before section resolution); writes get a tailored
/// rejection in [`validate_write_path`].
pub const CONFIG_SECTIONS: &[&str] = &[
    "agent",
    "a2a",
    "brain",
    "browser",
    "channels",
    "cron",
    "daemon",
    "database",
    "debug",
    "doctor",
    "image",
    "logging",
    "memory",
    "provider_registry",
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

    // `voice` reads like a section (the derived view) so people try to write
    // it; steer them at the real tables instead of a generic rejection (#1385
    // — before this, the write passed the gate and serde discarded the table).
    if head == "voice" {
        return Err(
            "'voice' is a derived, read-only view over providers.stt/providers.tts — \
             it cannot be written. Set the underlying keys instead (e.g. \
             providers.stt.model, providers.tts.voice)."
                .to_string(),
        );
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

/// Can a WRITE address `section`/`key` in the candidate document? (#83/#87)
///
/// Re-landed on the #1385 architecture: the section LIST is upstream's pinned
/// `CONFIG_SECTIONS` (drift-guarded), while the key dimension stays
/// struct-derived. `section` is the dotted path as actually written into the
/// document (the table path `write_item` navigated), `candidate` the
/// serialized document AFTER the key was inserted — so the check reflects the
/// locked live file, with no TOCTOU window. The candidate is deserialized
/// through `Config` with `serde_ignored`; the write is valid iff the struct
/// does not ignore a key at the target path. Map-valued sections
/// (`providers.custom.*`, `channels.telegram.groups.*`) never report ignored
/// keys — map contents are user-chosen, not schema.
///
/// Returns `Ok(())` or a message naming the unknown path, the likely intent,
/// and the fact that the file was not changed.
pub fn write_guard(section: &str, key: &str, candidate: &str) -> Result<(), String> {
    reject_bad_shape(section)?;
    let section = section.trim().trim_matches('.');
    let target = format!("{section}.{key}");
    let ignored = ignored_key_paths(candidate)?;
    let blocked = ignored
        .iter()
        .any(|p| p == &target || target.starts_with(&format!("{p}.")));
    if !blocked {
        return Ok(());
    }

    let head = section.split('.').next().unwrap_or(section);
    let suggestion = SECTION_PARENTS
        .iter()
        .find(|(child, _)| *child == head)
        .map(|(_, parent)| format!(" — did you mean '{parent}.{section}'?"))
        .or_else(|| suggest(head).map(|s| format!(" — did you mean '{s}'?")))
        .unwrap_or_default();
    Err(format!(
        "unknown config path '{target}': the Config struct has no such section or key{suggestion}. \
         Writing it would create a table/key serde ignores on load, so the value would silently \
         never apply (#1199). The file was NOT changed. Known sections: {}.",
        CONFIG_SECTIONS.join(", ")
    ))
}

/// Refuse section strings that would name junk tables in the document:
/// empty, padded, or double-dotted paths would create tables/key names the
/// struct ignores on load.
fn reject_bad_shape(section: &str) -> Result<(), String> {
    let trimmed = section.trim();
    if trimmed.is_empty() {
        return Err("config section is empty".to_string());
    }
    if trimmed != section {
        return Err(format!(
            "config section '{section}' has leading/trailing whitespace — it would name a table \
             the Config struct ignores on load. Use '{trimmed}'."
        ));
    }
    if section.split('.').any(|p| p.is_empty()) {
        return Err(format!(
            "config section '{section}' has an empty segment — it would name a table the Config \
             struct ignores on load. Use one dot between segments."
        ));
    }
    Ok(())
}

/// Deserialize `content` through `Config`, collecting the dotted path of
/// every key the struct ignores (the `serde_ignored` pass — the compiled
/// struct IS the registry, so "ignored" means "the app discards this on
/// load"). `Err` when `content` does not deserialize at all.
pub fn ignored_key_paths(content: &str) -> Result<Vec<String>, String> {
    let de = toml::Deserializer::new(content);
    let mut ignored: Vec<String> = Vec::new();
    let res: Result<Config, _> = serde_ignored::deserialize(de, |path| {
        ignored.push(path.to_string());
    });
    res.map_err(|e| e.to_string())?;
    Ok(ignored)
}

/// Nearest known section to `unknown` by edit distance, when close enough to
/// be a plausible typo (distance ≤ 2). `None` otherwise, so the caller falls
/// back to listing all known sections.
fn suggest(unknown: &str) -> Option<String> {
    let unknown = unknown.to_lowercase();
    CONFIG_SECTIONS
        .iter()
        .map(|s| (edit_distance(&unknown, s), *s))
        .filter(|(d, _)| *d <= 2)
        .min_by_key(|(d, _)| *d)
        .map(|(_, s)| s.to_string())
}

/// Classic Levenshtein distance — small, dependency-free, only used for
/// typo suggestions.
fn edit_distance(a: &str, b: &str) -> usize {
    if a == b {
        return 0;
    }
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            cur.push(
                (prev[j + 1] + 1)
                    .min(cur[j] + 1)
                    .min(prev[j] + usize::from(ca != cb)),
            );
        }
        prev = cur;
    }
    prev[b.len()]
}
