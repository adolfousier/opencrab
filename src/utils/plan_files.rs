//! Shared session plan file store: the single loader/saver for the plan
//! lifecycle engine (NoPlan / Editing / Active).
//!
//! Plan artifacts live under `~/.opencrabs/agents/session/`:
//! - `.opencrabs_plan_{session_id}.json`: the live store (status, title,
//!   checklist). The minimal pre-init Editing sidecar uses the same path.
//! - `.opencrabs_plan_{session_id}.md`: canonical design prose while the
//!   plan is post-init Editing; frozen against generic writes once Active.
//! - `archive/`: completed plans move here with a timestamp; there is no
//!   lingering live "Done" status.
//!
//! Legacy seven-status JSON is mapped on load (see [`PlanStatus`]'s
//! deserializer). Two terminal legacy statuses are resolved here, at the
//! file level, because they end the plan's life rather than describe it:
//! `Completed` archives silently and `Cancelled` deletes: both yield
//! `None` (NoPlan).

use crate::tui::plan::{PlanDocument, PlanStatus};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// `~/.opencrabs/agents/session/`: where session plan artifacts live.
pub fn session_dir() -> PathBuf {
    crate::config::opencrabs_home()
        .join("agents")
        .join("session")
}

/// `~/.opencrabs/agents/session/archive/`: where completed plans retire.
pub fn archive_dir() -> PathBuf {
    session_dir().join("archive")
}

/// Live plan JSON path for a session.
pub fn plan_json_path(session_id: Uuid) -> PathBuf {
    session_dir().join(format!(".opencrabs_plan_{session_id}.json"))
}

/// Session design markdown path (exists only after a design-track `init`).
pub fn plan_md_path(session_id: Uuid) -> PathBuf {
    session_dir().join(format!(".opencrabs_plan_{session_id}.md"))
}

/// The live plan-mode state of a session, derived from the files on disk.
///
/// This is the engine's source of truth: implementers must not treat
/// "file exists" as "live plan" (the pre-init flag is a first-class bit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanModeState {
    /// No plan artifacts and no durable pre-init flag.
    NoPlan,
    /// Plan-mode intent entered (`/plan` / soft-nudge) but `plan init` has
    /// not succeeded yet: minimal JSON flag only, no approvable `.md`.
    PreInitEditing,
    /// Design track after a successful `plan init`: `.md` + `.json`,
    /// `tasks` empty, design prose only.
    PostInitEditing,
    /// Checklist is live. A design `.md`, if present, is frozen.
    Active,
}

impl PlanModeState {
    /// Either Editing sub-state.
    pub fn is_editing(&self) -> bool {
        matches!(
            self,
            PlanModeState::PreInitEditing | PlanModeState::PostInitEditing
        )
    }
}

/// Derive the session's plan-mode state from disk.
pub fn plan_mode_state(session_id: Uuid) -> PlanModeState {
    let json = plan_json_path(session_id);
    if !json.exists() {
        return PlanModeState::NoPlan;
    }
    let Some(plan) = load_plan(session_id) else {
        return PlanModeState::NoPlan;
    };
    let md_exists = plan_md_path(session_id).exists();
    match plan.status {
        PlanStatus::Active => PlanModeState::Active,
        PlanStatus::Editing if plan.pre_init_editing && !md_exists => PlanModeState::PreInitEditing,
        PlanStatus::Editing if md_exists => PlanModeState::PostInitEditing,
        // Editing without an .md or a pre-init flag is a legacy draft (the
        // old seven-status world had no design track). load_plan normalizes
        // drafts with tasks to Active; an empty legacy draft gates nothing.
        PlanStatus::Editing => PlanModeState::NoPlan,
    }
}

/// Load the session plan, applying the legacy lifecycle rules:
///
/// - legacy `Completed` → silently archive both files, return `None`
/// - legacy `Cancelled` → delete both files, return `None`
/// - legacy draft-shaped checklists (Editing after the status map, tasks
///   non-empty, no `.md`, no pre-init flag) are normalized to `Active` in
///   memory: they were executable in the old world and stay executable.
/// - anything else parses through [`PlanStatus`]'s legacy string map.
pub fn load_plan(session_id: Uuid) -> Option<PlanDocument> {
    load_plan_from_path(&plan_json_path(session_id))
}

/// Maximum plan file size (10MB): guards every consumer of the loader.
pub const MAX_PLAN_FILE_SIZE: u64 = 10 * 1024 * 1024;

/// [`load_plan`] for callers that already hold the JSON path (TUI).
pub fn load_plan_from_path(path: &Path) -> Option<PlanDocument> {
    if let Ok(meta) = std::fs::metadata(path)
        && meta.len() > MAX_PLAN_FILE_SIZE
    {
        tracing::warn!(
            "Plan file too large ({} bytes) at {}; refusing to load",
            meta.len(),
            path.display()
        );
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    let raw: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Unreadable plan JSON at {}: {e}", path.display());
            return None;
        }
    };

    // Terminal legacy statuses end the plan's life at the file level.
    let raw_status = raw.get("status").and_then(|s| s.as_str()).unwrap_or("");
    match raw_status {
        "Completed" => {
            if let Err(e) = archive_plan_files(path) {
                tracing::warn!("Failed to archive completed plan: {e}");
            }
            return None;
        }
        "Cancelled" => {
            remove_plan_files(path);
            return None;
        }
        _ => {}
    }

    let mut plan: PlanDocument = match serde_json::from_value(raw) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("Failed to parse plan JSON at {}: {e}", path.display());
            return None;
        }
    };

    // Legacy checklist normalization: an old Draft/PendingApproval plan with
    // tasks was executable before the design/checklist split and must not be
    // trapped in Editing (there is no .md to approve).
    if plan.status == PlanStatus::Editing
        && !plan.pre_init_editing
        && !plan.tasks.is_empty()
        && !md_path_for(path).exists()
    {
        plan.status = PlanStatus::Active;
    }

    Some(plan)
}

/// Save the plan atomically (temp file + rename), writing the canonical
/// `"Editing"` / `"Active"` status strings.
pub fn save_plan(plan: &PlanDocument) -> std::io::Result<()> {
    let dir = session_dir();
    std::fs::create_dir_all(&dir)?;
    let path = plan_json_path(plan.session_id);
    let json = serde_json::to_string_pretty(plan)
        .map_err(|e| std::io::Error::other(format!("serialize plan: {e}")))?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &json)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// Mark the session as pre-init Editing: the user entered Plan-mode intent
/// but `plan init` has not succeeded yet. Durable (survives restart); the
/// sidecar is a minimal plan JSON with the flag set, empty tasks, and no
/// approvable content. Refused (Err) when a real plan is already live.
pub fn set_pre_init_editing(session_id: Uuid) -> std::io::Result<()> {
    match plan_mode_state(session_id) {
        PlanModeState::PostInitEditing | PlanModeState::Active => {
            return Err(std::io::Error::other(
                "a plan is already live for this session",
            ));
        }
        PlanModeState::NoPlan | PlanModeState::PreInitEditing => {}
    }
    let mut sidecar = PlanDocument::new(session_id, String::new(), String::new());
    sidecar.pre_init_editing = true;
    sidecar.status = PlanStatus::Editing;
    save_plan(&sidecar)
}

/// Whether the durable pre-init Editing flag is set for the session.
pub fn is_pre_init_editing(session_id: Uuid) -> bool {
    plan_mode_state(session_id) == PlanModeState::PreInitEditing
}

/// Archive the session's plan artifacts (`.json` and `.md`) under
/// `archive/` with a timestamp, returning the session to NoPlan.
pub fn archive_plan(session_id: Uuid) -> std::io::Result<()> {
    archive_plan_files(&plan_json_path(session_id))
}

fn archive_plan_files(json_path: &Path) -> std::io::Result<()> {
    let dir = archive_dir();
    std::fs::create_dir_all(&dir)?;
    let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let stem = json_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("plan")
        .trim_start_matches('.')
        .to_string();
    if json_path.exists() {
        std::fs::rename(json_path, dir.join(format!("{stem}-{ts}.json")))?;
    }
    let md = md_path_for(json_path);
    if md.exists() {
        std::fs::rename(&md, dir.join(format!("{stem}-{ts}.md")))?;
    }
    Ok(())
}

/// Delete the session's plan artifacts (or clear the pre-init sidecar),
/// returning the session to NoPlan. The engine half of Discard; command
/// wiring is the UX layer's.
pub fn discard_plan(session_id: Uuid) {
    remove_plan_files(&plan_json_path(session_id));
}

fn remove_plan_files(json_path: &Path) {
    if json_path.exists()
        && let Err(e) = std::fs::remove_file(json_path)
    {
        tracing::warn!("Failed to remove plan JSON {}: {e}", json_path.display());
    }
    let md = md_path_for(json_path);
    if md.exists()
        && let Err(e) = std::fs::remove_file(&md)
    {
        tracing::warn!("Failed to remove plan markdown {}: {e}", md.display());
    }
}

fn md_path_for(json_path: &Path) -> PathBuf {
    json_path.with_extension("md")
}

/// Create the session design `.md` with the light template B scaffold.
/// The model fills the sections with natural language; only the headings,
/// context labels, and step numbering are structural.
pub fn create_design_md(session_id: Uuid, title: &str) -> std::io::Result<PathBuf> {
    let dir = session_dir();
    std::fs::create_dir_all(&dir)?;
    let path = plan_md_path(session_id);
    let scaffold = format!(
        "# {title}\n\n\
         ## Context\n\
         - **Problem:** \n\
         - **Target state:** \n\
         - **Intent:** \n\n\
         ## Implementation steps\n\
         1. \n"
    );
    std::fs::write(&path, scaffold)?;
    Ok(path)
}

/// Sync the session `.md` body into the plan JSON `description` (the
/// Editing mirror). Tasks are never touched: Editing cannot persist a
/// checklist. Returns any template-section warnings (advisory only; a
/// missing section never blocks the write).
pub fn sync_md_to_json(session_id: Uuid) -> Vec<String> {
    let md = plan_md_path(session_id);
    let Ok(body) = std::fs::read_to_string(&md) else {
        return Vec::new();
    };
    let Some(mut plan) = load_plan(session_id) else {
        return Vec::new();
    };
    if plan.status != PlanStatus::Editing {
        return Vec::new();
    }
    plan.description = body.clone();
    plan.updated_at = chrono::Utc::now();
    if let Err(e) = save_plan(&plan) {
        tracing::warn!("Failed to mirror plan .md into JSON description: {e}");
    }
    template_section_warnings(&body)
}

/// Advisory light-template-B checks for the design `.md`: `## Context`
/// (with Problem / Target state / Intent) and at least one numbered
/// `## Implementation steps` entry are required before Approve.
pub fn template_section_warnings(md: &str) -> Vec<String> {
    let mut warnings = Vec::new();
    if !md.contains("## Context") {
        warnings.push("missing required `## Context` section".to_string());
    }
    for label in ["**Problem:**", "**Target state:**", "**Intent:**"] {
        let filled = md.lines().any(|l| {
            l.split_once(label)
                .is_some_and(|(_, rest)| !rest.trim().is_empty())
        });
        if !filled {
            warnings.push(format!("`{label}` needs non-empty text after the label"));
        }
    }
    if !md.contains("## Implementation steps") {
        warnings.push("missing required `## Implementation steps` section".to_string());
    } else {
        let has_step = md.lines().any(|l| {
            let t = l.trim_start();
            let rest = t.trim_start_matches(|c: char| c.is_ascii_digit());
            t.starts_with(|c: char| c.is_ascii_digit())
                && rest.starts_with('.')
                && !rest.trim_start_matches('.').trim().is_empty()
        });
        if !has_step {
            warnings.push("`## Implementation steps` needs at least one numbered step".to_string());
        }
    }
    warnings
}
