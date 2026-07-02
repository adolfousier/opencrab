//! Project directive-file discovery.
//!
//! Scans a project working directory for the directive / rule files that the
//! major AI coding agents use (Claude Code, Cursor, Windsurf, Cline, Gemini,
//! GitHub Copilot, OpenCode, and the cross-tool `AGENTS.md` convention) and
//! classifies each into one of three tiers so the system prompt can surface
//! them with the right guidance.
//!
//! The module is pure and filesystem-only: no logging, no globals. It takes an
//! already tilde-expanded `root` path and returns owned data, which keeps it
//! trivially testable and keeps `prompt_builder` thin.

use std::path::Path;

use crate::brain::skills::split_frontmatter;

/// When a discovered directive file should be applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectiveTier {
    /// Always relevant: plain root files, or rule files with no conditional
    /// frontmatter (Cursor `alwaysApply: true`, Claude/Cline rules without
    /// `paths`). Read whenever working in the project.
    Always,
    /// Scoped to files matching a glob / path pattern (Cursor `globs`,
    /// Claude/Cline `paths`). The payload is the normalized pattern list.
    Conditional(String),
    /// Applied by judgement from a `description` (Cursor `.mdc` with neither
    /// `alwaysApply` nor `globs`). The payload is the description text.
    OnDemand(String),
}

/// A directive file discovered under a project root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectiveFile {
    /// Path relative to the project root (e.g. `.cursor/rules/api.mdc`).
    pub rel_path: String,
    pub tier: DirectiveTier,
}

/// Root-level single-file directives, always relevant (plain markdown, no
/// frontmatter contract).
const ROOT_FILES: &[&str] = &[
    "AGENTS.md",
    "CLAUDE.md",
    "CLAUDE.local.md",
    "GEMINI.md",
    ".cursorrules",
    ".windsurfrules",
];

/// Single-file directives at a fixed sub-path, always relevant.
const NESTED_FILES: &[&str] = &[
    ".claude/CLAUDE.md",
    ".github/copilot-instructions.md",
    ".opencode/AGENTS.md",
];

/// Directory-based rule systems: `(dir, rule-file extensions)`. `.clinerules`
/// may also be a single file, which `discover` handles separately, but its
/// directory form and mtime are covered here. Kept in sync with the rule-dir
/// arms of `discover`; `directives_mtime` walks the same set.
const RULE_DIRS: &[(&str, &[&str])] = &[
    (".clinerules", &["md", "txt"]),
    (".claude/rules", &["md"]),
    (".cursor/rules", &["mdc"]),
];

/// How a rule directory's frontmatter maps to tiers.
enum FrontmatterKind {
    /// Cursor `.mdc`: `alwaysApply` (bool) / `globs` (list) / `description`.
    Cursor,
    /// Claude Code & Cline: `paths` (list) present means conditional, absent
    /// means always.
    Paths,
}

/// Discover and classify directive files under `root`.
///
/// Returns an empty vec when `root` is not a directory or nothing is found.
/// `root` must already be tilde-expanded to a real absolute path.
pub fn discover(root: &Path) -> Vec<DirectiveFile> {
    if !root.is_dir() {
        return Vec::new();
    }
    let mut found: Vec<DirectiveFile> = Vec::new();

    for name in ROOT_FILES {
        if root.join(name).is_file() {
            found.push(DirectiveFile {
                rel_path: (*name).to_string(),
                tier: DirectiveTier::Always,
            });
        }
    }
    for rel in NESTED_FILES {
        if root.join(rel).is_file() {
            found.push(DirectiveFile {
                rel_path: (*rel).to_string(),
                tier: DirectiveTier::Always,
            });
        }
    }

    // `.clinerules` may be a single file OR a directory of rule files.
    let clinerules = root.join(".clinerules");
    if clinerules.is_file() {
        found.push(DirectiveFile {
            rel_path: ".clinerules".to_string(),
            tier: DirectiveTier::Always,
        });
    } else if clinerules.is_dir() {
        collect_rule_dir(
            root,
            &clinerules,
            &["md", "txt"],
            &FrontmatterKind::Paths,
            &mut found,
        );
    }

    // `.claude/rules/**` — Claude Code modular rules.
    let claude_rules = root.join(".claude").join("rules");
    if claude_rules.is_dir() {
        collect_rule_dir(
            root,
            &claude_rules,
            &["md"],
            &FrontmatterKind::Paths,
            &mut found,
        );
    }

    // `.cursor/rules/**` — Cursor `.mdc` rules.
    let cursor_rules = root.join(".cursor").join("rules");
    if cursor_rules.is_dir() {
        collect_rule_dir(
            root,
            &cursor_rules,
            &["mdc"],
            &FrontmatterKind::Cursor,
            &mut found,
        );
    }

    found.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    found.dedup_by(|a, b| a.rel_path == b.rel_path);
    found
}

/// Newest modification time across every directive file under `root`, or
/// `None` when `root` is not a directory or holds no directive files.
///
/// Drives brain-rebuild staleness: when a directive file is added, edited, or
/// removed, its own mtime (or its parent rule dir's mtime, for add/remove)
/// advances, so the cached system prompt is rebuilt on the next turn. Cheap:
/// stats a fixed set of root and nested paths and globs the rule dirs, which
/// are tiny or absent in the common case.
pub fn directives_mtime(root: &Path) -> Option<std::time::SystemTime> {
    if !root.is_dir() {
        return None;
    }
    let mut newest: Option<std::time::SystemTime> = None;
    let mut consider = |p: &Path| {
        if let Ok(m) = std::fs::metadata(p).and_then(|md| md.modified()) {
            newest = Some(newest.map_or(m, |n| n.max(m)));
        }
    };
    for name in ROOT_FILES {
        consider(&root.join(name));
    }
    for rel in NESTED_FILES {
        consider(&root.join(rel));
    }
    for (dir, exts) in RULE_DIRS {
        let d = root.join(dir);
        // The dir's own mtime catches add/remove (and the single-file
        // `.clinerules` form); the walked files catch in-place edits.
        consider(&d);
        for ext in *exts {
            let Some(pat) = d
                .join("**")
                .join(format!("*.{ext}"))
                .to_str()
                .map(String::from)
            else {
                continue;
            };
            let Ok(paths) = glob::glob(&pat) else {
                continue;
            };
            for path in paths.flatten() {
                consider(&path);
            }
        }
    }
    newest
}

/// Recursively collect rule files with the given extensions from `dir`,
/// classifying each by its frontmatter.
fn collect_rule_dir(
    root: &Path,
    dir: &Path,
    exts: &[&str],
    kind: &FrontmatterKind,
    out: &mut Vec<DirectiveFile>,
) {
    for ext in exts {
        let pattern = dir.join("**").join(format!("*.{ext}"));
        let Some(pat) = pattern.to_str() else {
            continue;
        };
        let Ok(paths) = glob::glob(pat) else {
            continue;
        };
        for path in paths.flatten() {
            if !path.is_file() {
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            out.push(DirectiveFile {
                rel_path: rel,
                tier: classify(&path, kind),
            });
        }
    }
}

/// Read a rule file's frontmatter and decide its tier.
fn classify(path: &Path, kind: &FrontmatterKind) -> DirectiveTier {
    let raw = std::fs::read_to_string(path).unwrap_or_default();
    let fm = split_frontmatter(&raw).map(|(f, _)| f).unwrap_or("");
    match kind {
        FrontmatterKind::Cursor => {
            if frontmatter_bool(fm, "alwaysApply") == Some(true) {
                return DirectiveTier::Always;
            }
            if let Some(globs) = frontmatter_list(fm, "globs") {
                return DirectiveTier::Conditional(globs);
            }
            if let Some(desc) = frontmatter_scalar(fm, "description") {
                return DirectiveTier::OnDemand(desc);
            }
            // `.mdc` with empty / no frontmatter: best-effort always.
            DirectiveTier::Always
        }
        FrontmatterKind::Paths => match frontmatter_list(fm, "paths") {
            Some(paths) => DirectiveTier::Conditional(paths),
            None => DirectiveTier::Always,
        },
    }
}

/// Locate `key:` in a frontmatter block and return its inline value plus any
/// following indented `- item` block-list lines. Matches only an exact `key:`
/// (so `descriptionFoo:` never matches `description`).
fn field(fm: &str, key: &str) -> Option<(String, Vec<String>)> {
    let lines: Vec<&str> = fm.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let Some(rest) = line
            .trim_start()
            .strip_prefix(key)
            .and_then(|r| r.strip_prefix(':'))
        else {
            continue;
        };
        let inline = rest.trim().to_string();
        let mut block = Vec::new();
        if inline.is_empty() {
            for next in &lines[i + 1..] {
                let t = next.trim_start();
                if let Some(item) = t.strip_prefix("- ") {
                    block.push(item.trim().to_string());
                } else if t.is_empty() {
                    continue;
                } else {
                    break;
                }
            }
        }
        return Some((inline, block));
    }
    None
}

/// Parse a boolean frontmatter scalar (`key: true`).
fn frontmatter_bool(fm: &str, key: &str) -> Option<bool> {
    let (inline, _) = field(fm, key)?;
    match inline.trim().to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Parse a scalar frontmatter string (`description: ...`), stripping quotes.
/// Returns `None` when the field is absent or empty.
fn frontmatter_scalar(fm: &str, key: &str) -> Option<String> {
    let (inline, _) = field(fm, key)?;
    let v = clean_scalar(&inline);
    if v.is_empty() { None } else { Some(v) }
}

/// Parse a list frontmatter field into a `", "`-joined display string.
/// Handles inline `[a, b]` and block `- item` forms. Returns `None` when the
/// field is absent or resolves to no items.
fn frontmatter_list(fm: &str, key: &str) -> Option<String> {
    let (inline, block) = field(fm, key)?;
    let items: Vec<String> = if inline.is_empty() {
        block
            .iter()
            .map(|s| clean_scalar(s))
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        let inner = inline.trim().trim_start_matches('[').trim_end_matches(']');
        inner
            .split(',')
            .map(clean_scalar)
            .filter(|s| !s.is_empty())
            .collect()
    };
    if items.is_empty() {
        None
    } else {
        Some(items.join(", "))
    }
}

/// Trim whitespace and surrounding quotes from a scalar token.
fn clean_scalar(s: &str) -> String {
    s.trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_string()
}

/// Render the discovered directive files as a tiered, filenames-only prompt
/// section. Only non-empty tiers are emitted. `root_display` is the
/// tilde-collapsed project root for the header.
pub fn render(root_display: &str, files: &[DirectiveFile]) -> String {
    let mut always: Vec<&str> = Vec::new();
    let mut conditional: Vec<(&str, &str)> = Vec::new();
    let mut on_demand: Vec<(&str, &str)> = Vec::new();
    for f in files {
        match &f.tier {
            DirectiveTier::Always => always.push(&f.rel_path),
            DirectiveTier::Conditional(p) => conditional.push((&f.rel_path, p)),
            DirectiveTier::OnDemand(d) => on_demand.push((&f.rel_path, d)),
        }
    }

    let mut out = format!("--- Project Directive Files (in {}/) ---\n", root_display);
    out.push_str(
        "Directive / rule files from other AI coding tools were found in this project. \
         They hold project-specific conventions that take precedence over your defaults. \
         Load with `read_file` when relevant.\n",
    );

    if !always.is_empty() {
        out.push_str("\nAlways apply (read when working in this project):\n  ");
        out.push_str(&always.join(", "));
        out.push('\n');
    }
    if !conditional.is_empty() {
        out.push_str("\nConditional (read when touching matching files):\n");
        for (path, pat) in &conditional {
            out.push_str(&format!("  {} (matches: {})\n", path, pat));
        }
    }
    if !on_demand.is_empty() {
        out.push_str("\nOn-demand (read when the description matches your task):\n");
        for (path, desc) in &on_demand {
            out.push_str(&format!("  {}: \"{}\"\n", path, desc));
        }
    }
    out.push('\n');
    out
}
