//! #1317 — the skills signature that drives Telegram menu re-publication.
//!
//! Pure filesystem tests over temp roots: stability, add/edit/remove
//! detection, symlinked skills (the report's exact shape), project
//! overlays, and loader-shaped indifference to non-skill files.

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use tempfile::TempDir;

use crate::channels::telegram::menu_refresh::skills_signature_from;

const VALID_SKILL: &str = "---\nname: demo\ndescription: demo skill\n---\nbody\n";

fn write_skill(root: &Path, name: &str, body: &str) {
    let dir = root.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("SKILL.md"), body).unwrap();
}

/// Force a distinct mtime so edit detection does not depend on the
/// filesystem's timestamp granularity.
fn set_mtime(path: &Path, offset_secs: u64) {
    let file = fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() + Duration::from_secs(offset_secs))
        .unwrap();
}

#[test]
fn signature_is_stable_when_nothing_changes() {
    let user = TempDir::new().unwrap();
    let projects = TempDir::new().unwrap();
    write_skill(user.path(), "alpha", VALID_SKILL);

    let first = skills_signature_from(user.path(), projects.path());
    let second = skills_signature_from(user.path(), projects.path());
    assert_eq!(first, second, "unchanged dirs must hash identically");
}

#[test]
fn signature_changes_on_add_edit_remove() {
    let user = TempDir::new().unwrap();
    let projects = TempDir::new().unwrap();
    write_skill(user.path(), "alpha", VALID_SKILL);
    let base = skills_signature_from(user.path(), projects.path());

    // Add: a new skill dir with a SKILL.md.
    write_skill(user.path(), "beta", VALID_SKILL);
    let with_beta = skills_signature_from(user.path(), projects.path());
    assert_ne!(base, with_beta, "adding a skill must change the signature");

    // Edit: same path, new content + forced later mtime.
    let beta_md = user.path().join("beta").join("SKILL.md");
    fs::write(
        &beta_md,
        "---\nname: beta\ndescription: edited\n---\nnew body\n",
    )
    .unwrap();
    set_mtime(&beta_md, 10);
    let edited = skills_signature_from(user.path(), projects.path());
    assert_ne!(
        with_beta, edited,
        "editing a SKILL.md must change the signature"
    );

    // Remove: back to exactly the base skill set.
    fs::remove_dir_all(user.path().join("beta")).unwrap();
    let removed = skills_signature_from(user.path(), projects.path());
    assert_eq!(
        base, removed,
        "removing the added skill must restore the base signature"
    );
}

#[test]
fn signature_ignores_non_skill_files() {
    let user = TempDir::new().unwrap();
    let projects = TempDir::new().unwrap();
    write_skill(user.path(), "alpha", VALID_SKILL);
    let base = skills_signature_from(user.path(), projects.path());

    // Files the loader never reads must not affect the signature: a stray
    // file at the overlay root and a non-SKILL.md file inside a skill dir.
    fs::write(user.path().join("README.md"), "not a skill").unwrap();
    fs::write(user.path().join("alpha").join("notes.txt"), "not SKILL.md").unwrap();
    let polluted = skills_signature_from(user.path(), projects.path());
    assert_eq!(
        base, polluted,
        "non-skill files must not change the signature"
    );
}

#[test]
#[cfg(unix)]
fn signature_follows_symlinked_skill_dirs() {
    // The #1317 report shape: a skill symlinked into the overlay from
    // elsewhere. The signature must see it and track edits made to the
    // target, because metadata() resolves the symlink.
    let real = TempDir::new().unwrap();
    write_skill(real.path(), "linked", VALID_SKILL);
    let linked_md = real.path().join("linked").join("SKILL.md");
    set_mtime(&linked_md, 20);

    let user = TempDir::new().unwrap();
    let projects = TempDir::new().unwrap();
    let empty = skills_signature_from(user.path(), projects.path());

    std::os::unix::fs::symlink(real.path().join("linked"), user.path().join("linked")).unwrap();

    let with_link = skills_signature_from(user.path(), projects.path());
    assert_ne!(
        empty, with_link,
        "a symlinked skill must be visible to the signature"
    );

    // Editing the real target through the symlink's metadata must register.
    fs::write(
        &linked_md,
        "---\nname: linked\ndescription: edited\n---\nchanged\n",
    )
    .unwrap();
    set_mtime(&linked_md, 30);
    let after_edit = skills_signature_from(user.path(), projects.path());
    assert_ne!(
        with_link, after_edit,
        "editing a symlinked skill's target must change the signature"
    );
}

#[test]
fn signature_covers_project_overlays() {
    let user = TempDir::new().unwrap();
    let projects = TempDir::new().unwrap();

    let empty = skills_signature_from(user.path(), projects.path());

    let project_skills = projects.path().join("myproj").join("skills");
    write_skill(&project_skills, "proj-skill", VALID_SKILL);
    let with_project = skills_signature_from(user.path(), projects.path());
    assert_ne!(
        empty, with_project,
        "a project-overlay skill must be visible to the signature"
    );

    // A second project's skill must change it again (qualified names keep
    // same-named skills in different projects distinct).
    write_skill(
        &projects.path().join("other").join("skills"),
        "proj-skill",
        VALID_SKILL,
    );
    let two_projects = skills_signature_from(user.path(), projects.path());
    assert_ne!(
        with_project, two_projects,
        "same-named skills in different projects must not collide"
    );
}
