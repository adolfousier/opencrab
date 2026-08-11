//! Governance tests for the seed brain-file templates.
//!
//! These lock in the structure established while de-duplicating the templates:
//! one owner per rule, ownership-map headers, no personal/user file names, the
//! real service/daemon setup documented, and the dead VOICE.md / BOOTSTRAP.md
//! files gone (and unseeded). Without these guards the duplication and stale
//! references that an audit found in live workspaces would creep back.

use std::fs;
use std::path::Path;

const TEMPLATE_DIR: &str = "src/docs/reference/templates";

/// Rule-bearing templates that must carry an ownership-map header.
/// HEARTBEAT.md is excluded — it's a near-empty config file, not a rules file.
const OWNED_TEMPLATES: &[&str] = &[
    "SOUL.md",
    "USER.md",
    "AGENTS.md",
    "CODE.md",
    "TOOLS.md",
    "SECURITY.md",
    "MEMORY.md",
    "BOOT.md",
];

fn read_template(name: &str) -> String {
    let path = Path::new(TEMPLATE_DIR).join(name);
    fs::read_to_string(&path).unwrap_or_else(|_| panic!("template {name} must exist at {path:?}"))
}

// ── ownership-map headers ────────────────────────────────────────────────────

#[test]
fn every_rule_template_has_ownership_header() {
    for name in OWNED_TEMPLATES {
        let content = read_template(name);
        assert!(
            content.contains("> **Owns:**"),
            "{name} must open with a `> **Owns:** …` ownership-map header so RSI \
             and future edits know what this file is the single source of truth for"
        );
    }
}

// ── hard rules live in AGENTS (always-loaded), SOUL is personality ───────────

#[test]
fn enforced_gates_live_in_agents_not_soul() {
    let soul = read_template("SOUL.md");
    let agents = read_template("AGENTS.md");
    // AGENTS owns the enforced permission/commit gates (it's always-loaded).
    assert!(
        agents.contains("create PRs only") && agents.contains("NEVER DO WITHOUT EXPLICIT APPROVAL"),
        "AGENTS.md must own the enforced hard rules / permission gates"
    );
    // SOUL no longer carries the full gate list — it just points to AGENTS.
    assert!(
        !soul.contains("create PRs only"),
        "SOUL.md must NOT duplicate the commit/push gate — it lives in AGENTS.md"
    );
    assert!(
        soul.contains("AGENTS.md"),
        "SOUL.md must point to AGENTS.md for the hard rules"
    );
}

#[test]
fn preamble_and_rsi_route_hard_rules_to_agents() {
    use crate::brain::prompt_builder::BRAIN_PREAMBLE;
    assert!(
        BRAIN_PREAMBLE.contains("AGENTS.md")
            && BRAIN_PREAMBLE.contains("hard rules")
            && BRAIN_PREAMBLE.contains("Always-loaded"),
        "preamble must route hard rules to the always-loaded AGENTS.md"
    );
    const RSI_SRC: &str = include_str!("../brain/rsi.rs");
    assert!(
        RSI_SRC.contains("ALWAYS-LOADED") && RSI_SRC.contains("hard rules"),
        "RSI taxonomy must route learned hard rules to the always-loaded AGENTS.md"
    );
}

// ── commands & skills discovery ──────────────────────────────────────────────

#[test]
fn agents_documents_command_and_skill_discovery() {
    let agents = read_template("AGENTS.md");
    assert!(
        agents.contains("Commands & Skills") && agents.contains("slash_command"),
        "AGENTS.md must document how to discover/run user commands & skills"
    );
    assert!(
        agents.contains("Available Commands & Skills"),
        "AGENTS.md must reference the always-injected live commands/skills index"
    );
}

#[test]
fn rsi_notes_new_commands_skills_are_auto_discoverable() {
    const RSI_SRC: &str = include_str!("../brain/rsi.rs");
    assert!(
        RSI_SRC.contains("Available Commands & Skills")
            || RSI_SRC.contains("discoverable automatically"),
        "RSI must note that a newly-applied command/skill is auto-discoverable via \
         the injected index, so it isn't re-documented in a brain file"
    );
}

// ── single source of truth (no rule duplicated across files) ─────────────────

#[test]
fn rust_first_policy_lives_only_in_code_md() {
    // The full policy text is the distinctive phrase. It must appear in CODE.md
    // and NOWHERE else — AGENTS.md and BOOT.md only carry a pointer.
    const FULL_TEXT: &str = "always prioritize Rust-based crates";
    assert!(
        read_template("CODE.md").contains(FULL_TEXT),
        "CODE.md must own the full Rust-First Policy"
    );
    for other in ["AGENTS.md", "BOOT.md"] {
        let c = read_template(other);
        assert!(
            !c.contains(FULL_TEXT),
            "{other} must NOT duplicate the Rust-First Policy text — point to CODE.md"
        );
        assert!(
            c.contains("Rust-First") && c.contains("CODE.md"),
            "{other} must keep a pointer to CODE.md for the Rust-First Policy"
        );
    }
}

#[test]
fn upgrade_procedure_lives_in_boot_md() {
    // The build-from-source upgrade block belongs to BOOT.md; AGENTS.md points.
    assert!(
        read_template("BOOT.md").contains("cargo build --release"),
        "BOOT.md must own the upgrade procedure"
    );
    let agents = read_template("AGENTS.md");
    assert!(
        agents.contains("Upgrading") && agents.contains("BOOT.md"),
        "AGENTS.md must point to BOOT.md for upgrading, not duplicate the steps"
    );
}

/// Memory-save triggers live in the ALWAYS-LOADED file, with BOOT.md pointing
/// at it (#1003).
///
/// This assertion used to run the other way, and the other way was wrong.
/// Ownership had been assigned by topic (BOOT.md owns runtime
/// self-maintenance) while loading was assigned by cost (BOOT.md is
/// contextual), and nobody reconciled the two. A save trigger fires
/// MID-SESSION, on an arbitrary turn, when the user corrects you. BOOT.md is
/// not in context at that moment unless something loaded it.
///
/// Automatic recall cannot cover the gap either: a correction is short and
/// conversational, which is exactly the message shape BM25 recall stays silent
/// on by design (#996). Same reasoning that keeps enforced gates in AGENTS.md
/// instead of behind retrieval.
///
/// The symptom was a live AGENTS.md that restated the whole trigger list inline
/// while also carrying a pointer calling BOOT.md the single source of truth.
/// That "duplication" was load-bearing: it was the only copy ever in context.
#[test]
fn memory_save_triggers_live_in_the_always_loaded_file() {
    let agents = read_template("AGENTS.md");
    let boot = read_template("BOOT.md");

    assert!(
        agents.contains("What triggers a save to"),
        "AGENTS.md must own the memory-save triggers: it is the always-loaded \
         file, and a trigger that fires mid-session has to be in context then"
    );
    assert!(
        boot.contains("AGENTS.md") && boot.contains("Auto-Save Important Memories"),
        "BOOT.md must point at AGENTS.md for the triggers rather than restate them"
    );
    assert!(
        !boot.contains("What triggers a save to"),
        "BOOT.md must not keep a second copy of the trigger list"
    );
    assert!(
        !boot.contains("The single home for \"when to save memory\""),
        "BOOT.md's ownership header must stop claiming the memory triggers"
    );
}

// ── no personal / user file names anywhere in the templates ──────────────────

#[test]
fn templates_reference_no_personal_user_files() {
    for name in OWNED_TEMPLATES.iter().chain(["HEARTBEAT.md"].iter()) {
        let content = read_template(name);
        for banned in ["VOICE.md", "AGENTVERSE.md"] {
            assert!(
                !content.contains(banned),
                "{name} must not reference the user file {banned} — those are \
                 arbitrary user-created files, not canonical brain files"
            );
        }
    }
}

// ── the real service / daemon setup is documented ────────────────────────────

#[test]
fn boot_md_documents_service_and_daemon_setup() {
    let boot = read_template("BOOT.md");
    assert!(
        boot.contains("opencrabs service install"),
        "BOOT.md must document `opencrabs service install` (systemd/launchd)"
    );
    assert!(
        boot.contains("opencrabs daemon"),
        "BOOT.md must mention the `opencrabs daemon` process the unit runs"
    );
}

// ── deleted templates are gone and unseeded ──────────────────────────────────

#[test]
fn deleted_templates_do_not_exist() {
    for gone in ["VOICE.md", "BOOTSTRAP.md"] {
        assert!(
            !Path::new(TEMPLATE_DIR).join(gone).exists(),
            "{gone} was removed as redundant — it must not come back"
        );
    }
}

#[test]
fn boot_md_is_seeded_and_dead_files_are_not() {
    use crate::tui::onboarding::TEMPLATE_FILES;
    assert!(
        TEMPLATE_FILES.iter().any(|(n, _)| *n == "BOOT.md"),
        "BOOT.md must be seeded so its on-demand load (and AGENTS.md's pointers \
         to it) resolve for fresh users"
    );
    for gone in ["BOOTSTRAP.md", "VOICE.md"] {
        assert!(
            !TEMPLATE_FILES.iter().any(|(n, _)| *n == gone),
            "{gone} must not be seeded"
        );
    }
}

// ── the ownership model reaches the system prompt and RSI ────────────────────

#[test]
fn preamble_carries_brain_file_ownership_map() {
    use crate::brain::prompt_builder::BRAIN_PREAMBLE;
    assert!(
        BRAIN_PREAMBLE.contains("BRAIN FILE OWNERSHIP"),
        "the system preamble must teach the agent the brain-file ownership map"
    );
    for f in [
        "SOUL.md",
        "MEMORY.md",
        "CODE.md",
        "TOOLS.md",
        "SECURITY.md",
        "BOOT.md",
    ] {
        assert!(
            BRAIN_PREAMBLE.contains(f),
            "preamble ownership map must name {f}"
        );
    }
    assert!(
        BRAIN_PREAMBLE.contains("never") && BRAIN_PREAMBLE.contains("duplicat"),
        "preamble must state that a rule is never duplicated across files"
    );
}

#[test]
fn rsi_taxonomy_routes_to_all_core_brain_files() {
    // Source-level check: the RSI improvement prompt must route to each core
    // brain file — including BOOT.md, which took over memory-save triggers,
    // upgrade, and service setup — and forbid cross-file duplication.
    const RSI_SRC: &str = include_str!("../brain/rsi.rs");
    for f in [
        "SOUL.md",
        "USER.md",
        "MEMORY.md",
        "AGENTS.md",
        "CODE.md",
        "TOOLS.md",
        "SECURITY.md",
        "BOOT.md",
    ] {
        assert!(
            RSI_SRC.contains(f),
            "RSI Target File Taxonomy must route improvements to {f}"
        );
    }
    assert!(
        RSI_SRC.contains("One kind of content per file"),
        "RSI must state the one-kind-per-file ownership principle"
    );
}

#[test]
fn memory_index_excludes_dead_files() {
    use crate::memory::BRAIN_FILES;
    for gone in ["BOOTSTRAP.md", "VOICE.md"] {
        assert!(
            !BRAIN_FILES.contains(&gone),
            "memory index must not list the removed {gone}"
        );
    }
}

// ── dead brain files are not promised by user-facing docs (#991) ─────────────

/// Files that were removed from the brain-file set. See the Removed Files
/// ledger in `src/docs/reference/BRAIN_CONSTITUTION.md`.
const DEAD_BRAIN_FILES: &[&str] = &["IDENTITY.md", "VOICE.md", "BOOTSTRAP.md"];

/// User-facing docs must not promise a brain file that no longer exists.
///
/// `deleted_templates_do_not_exist` above stops the template coming back, but
/// nothing stopped the docs describing it. The setup guide told users their
/// workspace is created with a starter `IDENTITY.md` and README diagrammed it
/// in the workspace tree, so a workspace built by following the docs carried a
/// file no code path reads, and anything written into it was silently inert.
///
/// `BRAIN_CONSTITUTION.md` is deliberately excluded: it is the ledger that
/// records these deletions, so naming them is its job. CHANGELOG is excluded
/// for the same reason, it is history.
#[test]
fn user_facing_docs_do_not_reference_dead_brain_files() {
    let mut checked = 0;
    for path in ["README.md", "src/docs/start", TEMPLATE_DIR] {
        for file in markdown_files(Path::new(path)) {
            if file.ends_with("BRAIN_CONSTITUTION.md") {
                continue;
            }
            let content = fs::read_to_string(&file).expect("doc reads");
            for dead in DEAD_BRAIN_FILES {
                assert!(
                    !content.contains(dead),
                    "{} references {dead}, which was removed from the brain-file set. \
                     Docs that promise a dead brain file produce workspaces carrying a \
                     file nothing reads.",
                    file.display()
                );
            }
            checked += 1;
        }
    }
    assert!(checked > 0, "scanned no docs, the paths must be wrong");
}

/// Every markdown file at or under `path` (a file yields just itself).
fn markdown_files(path: &Path) -> Vec<std::path::PathBuf> {
    if path.is_file() {
        return vec![path.to_path_buf()];
    }
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(path) else {
        return out;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            out.extend(markdown_files(&p));
        } else if p.extension().is_some_and(|e| e == "md") {
            out.push(p);
        }
    }
    out
}

// ── operating rules reach users as syncable sections (#992) ──────────────────

/// Operating rules that must exist in SOUL.md as their own `## ` sections.
const SOUL_RULE_SECTIONS: &[&str] = &[
    "## Your Role",
    "## Operating Rules",
    "## Epistemic Honesty",
    "## Never Assume, Verify",
    "## Fix, Don't Narrate",
];

/// Each operating rule is a TOP-LEVEL `## ` section, not prose folded into an
/// existing one.
///
/// This is a delivery constraint, not a style preference. `rsi_sync` merges
/// brain files by section: `extract_new_sections` appends only `## ` sections
/// the local copy lacks and never rewrites existing prose. Fold these rules
/// into `## Core Truths` and sync sees a section that already exists locally,
/// skips it, and every existing install receives nothing. Only brand-new
/// profiles would ever get them.
#[test]
fn soul_operating_rules_are_their_own_sections() {
    let soul = read_template("SOUL.md");
    for heading in SOUL_RULE_SECTIONS {
        assert!(
            soul.lines().any(|l| l.trim_end() == *heading),
            "SOUL.md must carry `{heading}` as a top-level section on its own line. \
             Folded into another section, rsi_sync will never deliver it to an \
             existing install."
        );
    }
}

/// SOUL owns the posture, AGENTS owns the mechanism, neither restates the other.
#[test]
fn epistemic_posture_and_protocol_do_not_duplicate() {
    let soul = read_template("SOUL.md");
    let agents = read_template("AGENTS.md");

    // The distinctive posture line belongs to SOUL alone.
    const SNAPSHOT: &str = "snapshots go stale";
    assert!(
        soul.contains(SNAPSHOT),
        "SOUL.md must own the Never Assume, Verify posture"
    );
    assert!(
        !agents.contains(SNAPSHOT),
        "AGENTS.md must not duplicate the posture text, it owns the protocol"
    );

    // The tracking mechanism belongs to AGENTS alone.
    assert!(
        agents.contains("## Epistemic Protocol") && agents.contains("Decay:"),
        "AGENTS.md must own the epistemic tracking protocol"
    );
    assert!(
        !soul.contains("Decay:"),
        "SOUL.md must not duplicate the protocol, it points at the posture only"
    );
}

/// SOUL says what the agent is FOR, not only how it sounds.
#[test]
fn soul_defines_a_role_not_only_a_voice() {
    let soul = read_template("SOUL.md");
    assert!(
        soul.contains("protect the production environment")
            && soul.contains("plan before executing"),
        "SOUL.md must state the operating posture (protect production, plan first), \
         not just the voice. A template that defines a tone with no job ships \
         personality without judgement."
    );
}
