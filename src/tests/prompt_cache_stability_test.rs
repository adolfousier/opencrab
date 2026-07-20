//! Prompt-cache stability of the system brain (#657).
//!
//! The system brain is the cached prefix (providers stamp cache_control on the
//! whole system message). A per-second timestamp in Runtime Info changed the
//! prefix every request, so the cache never hit and the provider re-prefilled
//! the full context every turn. Runtime Info must carry date granularity only,
//! so the cached prefix stays byte-stable across turns and same-model sessions.

use crate::brain::prompt_builder::{BrainLoader, RuntimeInfo};
use tempfile::TempDir;

fn loader() -> (TempDir, BrainLoader) {
    let dir = TempDir::new().expect("tempdir");
    let loader = BrainLoader::new(dir.path().to_path_buf());
    (dir, loader)
}

fn runtime_info() -> RuntimeInfo {
    RuntimeInfo {
        model: Some("test-model".to_string()),
        provider: Some("test-provider".to_string()),
        working_directory: Some("~/srv/rs/opencrabs".to_string()),
    }
}

/// A rough `HH:MM:SS` detector — three colon-separated 2-digit runs.
fn contains_seconds_time(s: &str) -> bool {
    s.split_whitespace().any(|tok| {
        let parts: Vec<&str> = tok.split(':').collect();
        parts.len() == 3
            && parts
                .iter()
                .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_digit()))
    })
}

#[test]
fn full_brain_runtime_info_uses_stable_date_only() {
    let (_dir, loader) = loader();
    let brain = loader.build_system_brain(Some(&runtime_info()));
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

    assert!(
        brain.contains(&format!("Current date: {today} (UTC)")),
        "expected a stable date line; runtime info was:\n{}",
        brain
            .lines()
            .filter(|l| l.contains("date") || l.contains("Runtime"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(
        !contains_seconds_time(&brain),
        "a per-second timestamp is back in the cached system prefix (#657)"
    );
}

#[test]
fn core_brain_runtime_info_uses_stable_date_only() {
    let (_dir, loader) = loader();
    let brain = loader.build_core_brain(Some(&runtime_info()));
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

    assert!(
        brain.contains(&format!("Current date: {today} (UTC)")),
        "core brain must render the stable date line too"
    );
    assert!(
        !contains_seconds_time(&brain),
        "core brain leaked a per-second timestamp (#657)"
    );
}

#[test]
fn full_brain_prefix_is_byte_stable_across_consecutive_builds() {
    // The concrete cache guarantee: two builds back to back are identical, so
    // the cached prefix a provider keyed on the first request still matches the
    // second. A per-second timestamp broke this.
    let (_dir, loader) = loader();
    let a = loader.build_system_brain(Some(&runtime_info()));
    let b = loader.build_system_brain(Some(&runtime_info()));
    assert_eq!(
        a, b,
        "system brain prefix drifted between two builds (#657)"
    );
}
