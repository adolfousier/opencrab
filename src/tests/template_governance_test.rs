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

#[test]
fn memory_save_triggers_consolidated_in_boot_with_agents_pointer() {
    assert!(
        read_template("BOOT.md").contains("Auto-Save Important Memories"),
        "BOOT.md must own the memory-save triggers (Auto-Save section)"
    );
    assert!(
        read_template("AGENTS.md").contains("BOOT.md → Auto-Save"),
        "AGENTS.md must point at BOOT.md for the memory-save triggers"
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
