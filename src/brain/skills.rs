//! Skill loader — embedded built-in skills with user-directory overlay.
//!
//! Skills are workflow templates: a `name`, a `description` (read by the
//! LLM to decide when to invoke), and a `body` (the actual instructions).
//! Format follows the de-facto `SKILL.md` convention used by Claude Code,
//! Anthropic managed agents, and OpenClaw — YAML frontmatter at the top
//! of the file, followed by the prompt body.
//!
//! Layout:
//!
//! ```text
//! ~/.opencrabs/skills/
//! └── <skill-name>/
//!     └── SKILL.md          ← user-owned, overrides any built-in of the same name
//! ```
//!
//! The repo ships a curated set of built-ins under
//! `src/docs/reference/templates/skills/<name>/SKILL.md`, embedded at
//! compile time via `include_str!`. The user directory at
//! `~/.opencrabs/skills/` is purely user-owned (per `TOOLS.md`); writes
//! never come from the binary.
//!
//! ## Resolution order
//!
//! 1. `~/.opencrabs/skills/<name>/SKILL.md` — user override
//! 2. embedded built-in
//!
//! A user file with a malformed frontmatter falls back to the built-in
//! (with a warning), so a broken local edit cannot brick the skill.
//!
//! ## Frontmatter
//!
//! ```markdown
//! ---
//! name: security-audit
//! description: Comprehensive security and CVE audit. Triggers on ...
//! ---
//!
//! Body of the skill...
//! ```
//!
//! Only `name` and `description` are recognised today. Other keys are
//! preserved for forward-compat but ignored.

use std::path::PathBuf;

/// Compile-time table of built-in skills shipped with the binary.
///
/// To add a new built-in, drop a `SKILL.md` under
/// `src/docs/reference/templates/skills/<name>/` and add a line here.
const BUILTIN_SKILLS: &[(&str, &str)] = &[
    (
        "cost-estimate",
        include_str!("../docs/reference/templates/skills/cost-estimate/SKILL.md"),
    ),
    (
        "security-audit",
        include_str!("../docs/reference/templates/skills/security-audit/SKILL.md"),
    ),
    (
        "repo-audit",
        include_str!("../docs/reference/templates/skills/repo-audit/SKILL.md"),
    ),
    (
        "opencli",
        include_str!("../docs/reference/templates/skills/opencli/SKILL.md"),
    ),
    (
        "browser-cdp",
        include_str!("../docs/reference/templates/skills/browser-cdp/SKILL.md"),
    ),
    (
        "a2a-gateway",
        include_str!("../docs/reference/templates/skills/a2a-gateway/SKILL.md"),
    ),
    (
        "dynamic-tools",
        include_str!("../docs/reference/templates/skills/dynamic-tools/SKILL.md"),
    ),
];

/// Where this skill came from. Used by the TUI to badge built-ins
/// differently from user-installed ones in the autocomplete dropdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillSource {
    Builtin,
    User,
}

#[derive(Debug, Clone)]
pub struct Skill {
    /// Slug used to invoke the skill (`/security-audit` → `"security-audit"`).
    pub name: String,
    /// Precomputed `/<name>` form, for autocomplete display and sort
    /// comparisons against built-in / user-command names that already
    /// carry the leading slash.
    pub slash_name: String,
    /// One-line summary the LLM reads to decide when to invoke.
    pub description: String,
    /// Prompt body (everything after the closing `---`, trimmed).
    pub body: String,
    pub source: SkillSource,
}

impl Skill {
    /// Parse a `SKILL.md` blob into a `Skill`. Returns `Err` if the
    /// frontmatter is missing required fields.
    pub fn parse(name: &str, raw: &str, source: SkillSource) -> Result<Self, String> {
        // Normalise CRLF → LF and strip BOM so the line-walking logic
        // below has a single shape to handle.
        let raw = raw.strip_prefix('\u{FEFF}').unwrap_or(raw);
        let normalised = raw.replace("\r\n", "\n");
        let (frontmatter, body) = split_frontmatter(&normalised)
            .ok_or_else(|| format!("skill '{name}': missing or malformed frontmatter"))?;

        let mut fm_name: Option<String> = None;
        let mut fm_description: Option<String> = None;

        for line in frontmatter.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once(':') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim().trim_matches('"').trim_matches('\'');
            match key {
                "name" => fm_name = Some(value.to_string()),
                "description" => fm_description = Some(value.to_string()),
                _ => {}
            }
        }

        let resolved_name = fm_name.unwrap_or_else(|| name.to_string());
        let description = fm_description
            .ok_or_else(|| format!("skill '{name}': frontmatter missing 'description'"))?;
        let slash_name = format!("/{resolved_name}");

        Ok(Self {
            name: resolved_name,
            slash_name,
            description,
            body: body.trim().to_string(),
            source,
        })
    }
}

/// Split a `SKILL.md` blob into (frontmatter, body).
///
/// Caller is expected to have normalised line endings to `\n` and stripped
/// any BOM. The opening fence must be the very first line (`---\n`);
/// the closing fence is the next standalone `---` line. Returns `None`
/// if either fence is missing.
fn split_frontmatter(raw: &str) -> Option<(&str, &str)> {
    let after_open = raw.strip_prefix("---\n")?;

    // Closing fence: a line that is exactly "---". Find its byte offset
    // by walking lines and tracking position — `len() + 1` is correct
    // here because we operate on LF-only input.
    let close_idx = after_open
        .lines()
        .scan(0usize, |acc, line| {
            let start = *acc;
            *acc += line.len() + 1;
            Some((start, line))
        })
        .find(|(_, line)| line.trim() == "---")
        .map(|(idx, _)| idx)?;

    let frontmatter = &after_open[..close_idx];
    // Skip past "---\n" to the body (or end-of-string for an empty body).
    let body_start = (close_idx + 4).min(after_open.len());
    let body = &after_open[body_start..];
    Some((frontmatter, body))
}

/// User skills directory: `~/.opencrabs/skills/`.
fn user_skills_dir() -> PathBuf {
    crate::config::opencrabs_home().join("skills")
}

/// Load every available skill (built-ins + user overlays).
///
/// User skills override built-ins of the same name. Skills with broken
/// frontmatter are skipped with a warning rather than aborting the load.
pub fn load_all_skills() -> Vec<Skill> {
    let mut by_name: std::collections::BTreeMap<String, Skill> = std::collections::BTreeMap::new();

    // 1. Built-ins
    for (name, raw) in BUILTIN_SKILLS {
        match Skill::parse(name, raw, SkillSource::Builtin) {
            Ok(skill) => {
                by_name.insert(skill.name.clone(), skill);
            }
            Err(e) => {
                tracing::error!("skills: built-in '{name}' failed to parse: {e}");
            }
        }
    }

    // 2. User overlay
    let dir = user_skills_dir();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let skill_path = path.join("SKILL.md");
            if !skill_path.exists() {
                continue;
            }
            let raw = match std::fs::read_to_string(&skill_path) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(
                        "skills: failed to read user skill '{name}' at {}: {e}",
                        skill_path.display()
                    );
                    continue;
                }
            };
            match Skill::parse(name, &raw, SkillSource::User) {
                Ok(skill) => {
                    by_name.insert(skill.name.clone(), skill);
                }
                Err(e) => {
                    tracing::warn!("skills: user skill '{name}' has bad frontmatter: {e}");
                }
            }
        }
    }

    by_name.into_values().collect()
}

/// Look up a single skill by name, applying the same resolution rules as
/// `load_all_skills` (user overlay wins).
pub fn resolve_skill(name: &str) -> Option<Skill> {
    load_all_skills().into_iter().find(|s| s.name == name)
}
