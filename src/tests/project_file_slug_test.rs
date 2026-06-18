//! Tests for `slugify_project_name` — the project-name → directory-slug used
//! to archive a session's files under `~/.opencrabs/projects/<slug>/files/`.

use crate::services::file::slugify_project_name;

#[test]
fn lowercases_and_keeps_alphanumerics() {
    assert_eq!(slugify_project_name("DevOps"), "devops");
    assert_eq!(slugify_project_name("Project42"), "project42");
}

#[test]
fn collapses_non_alphanumerics_to_single_dash() {
    assert_eq!(slugify_project_name("My Cool Project"), "my-cool-project");
    assert_eq!(slugify_project_name("a  --  b"), "a-b");
    assert_eq!(
        slugify_project_name("Client: Acme, Inc."),
        "client-acme-inc"
    );
}

#[test]
fn trims_leading_and_trailing_dashes() {
    assert_eq!(slugify_project_name("  spaced  "), "spaced");
    assert_eq!(slugify_project_name("!!!edges!!!"), "edges");
}

#[test]
fn empty_or_symbol_only_falls_back() {
    assert_eq!(slugify_project_name(""), "project");
    assert_eq!(slugify_project_name("***"), "project");
}
