//! Tests for issue #220: project-scoped skill resolution.
//!
//! Verifies that `load_all_skills()` picks up skills placed under
//! `~/.opencrabs/projects/<slug>/skills/<name>/SKILL.md` in addition to
//! the built-in and user profile layers.

use crate::brain::skills::{SkillSource, load_all_skills, resolve_skill};
use crate::config::profile::{home_for_profile, with_profile_home_async};
use crate::services::ProjectService;
use crate::services::file::slugify_project_name;

/// Helper: create a throwaway profile name so disk state is isolated.
fn throwaway_profile() -> String {
    format!("test-proj-skills-{}", uuid::Uuid::new_v4())
}

/// A project skill placed under `projects/<slug>/skills/` is found by
/// `load_all_skills()`.
#[tokio::test]
async fn project_skill_is_discovered() {
    let profile = throwaway_profile();

    let out = with_profile_home_async(Some(&profile), async {
        let skill_content = "\
---
name: payment-flow
description: Process payment via Stripe
---

Use Stripe API to charge the customer.
";

        let projects_dir = ProjectService::projects_dir();
        let slug = slugify_project_name("My Project");
        let skill_dir = projects_dir.join(&slug).join("skills").join("payment-flow");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), skill_content).unwrap();

        load_all_skills()
    })
    .await;

    let _ = std::fs::remove_dir_all(home_for_profile(Some(&profile)));

    let found = out.iter().find(|s| s.name == "payment-flow");
    assert!(
        found.is_some(),
        "project skill 'payment-flow' should be discovered, got: {:?}",
        out.iter().map(|s| &s.name).collect::<Vec<_>>()
    );
    let skill = found.unwrap();
    assert_eq!(skill.description, "Process payment via Stripe");
    assert!(skill.body.contains("Stripe API"));
}

/// A project skill is overridden by a user profile skill of the same name
/// (user overlay wins over project overlay).
#[tokio::test]
async fn user_skill_overrides_project_skill() {
    let profile = throwaway_profile();

    let out = with_profile_home_async(Some(&profile), async {
        let project_skill = "\
---
name: deploy
description: Project deploy instructions
---

Project-level deploy.
";
        let user_skill = "\
---
name: deploy
description: User deploy override
---

User-level deploy.
";

        // Project skill
        let projects_dir = ProjectService::projects_dir();
        let slug = slugify_project_name("TestProj");
        let skill_dir = projects_dir.join(&slug).join("skills").join("deploy");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), project_skill).unwrap();

        // User skill
        let home = crate::config::opencrabs_home();
        let user_skill_dir = home.join("skills").join("deploy");
        std::fs::create_dir_all(&user_skill_dir).unwrap();
        std::fs::write(user_skill_dir.join("SKILL.md"), user_skill).unwrap();

        load_all_skills()
    })
    .await;

    let _ = std::fs::remove_dir_all(home_for_profile(Some(&profile)));

    let deploy = out.iter().find(|s| s.name == "deploy");
    assert!(deploy.is_some(), "deploy skill should exist");
    assert_eq!(
        deploy.unwrap().description,
        "User deploy override",
        "user profile skill should override project skill"
    );
}

/// Built-in skills still resolve when no project or user overlays exist.
#[test]
fn builtins_still_resolve_without_overlays() {
    let skill = resolve_skill("security-audit");
    assert!(skill.is_some());
    assert_eq!(skill.unwrap().source, SkillSource::Builtin);
}

/// Multiple projects each contribute their own skills.
#[tokio::test]
async fn multiple_projects_skills_merge() {
    let profile = throwaway_profile();

    let out = with_profile_home_async(Some(&profile), async {
        let projects_dir = ProjectService::projects_dir();

        // Project A: billing skill
        let slug_a = slugify_project_name("ProjectA");
        let dir_a = projects_dir.join(&slug_a).join("skills").join("billing");
        std::fs::create_dir_all(&dir_a).unwrap();
        std::fs::write(
            dir_a.join("SKILL.md"),
            "---\nname: billing\ndescription: Billing system\n---\nBilling body.\n",
        )
        .unwrap();

        // Project B: analytics skill
        let slug_b = slugify_project_name("ProjectB");
        let dir_b = projects_dir.join(&slug_b).join("skills").join("analytics");
        std::fs::create_dir_all(&dir_b).unwrap();
        std::fs::write(
            dir_b.join("SKILL.md"),
            "---\nname: analytics\ndescription: Analytics pipeline\n---\nAnalytics body.\n",
        )
        .unwrap();

        load_all_skills()
    })
    .await;

    let _ = std::fs::remove_dir_all(home_for_profile(Some(&profile)));

    let names: Vec<&str> = out.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"billing"), "billing skill from ProjectA");
    assert!(
        names.contains(&"analytics"),
        "analytics skill from ProjectB"
    );
}

/// A project skill with bad frontmatter is silently skipped, not a crash.
#[tokio::test]
async fn bad_project_skill_is_skipped() {
    let profile = throwaway_profile();

    let out = with_profile_home_async(Some(&profile), async {
        let projects_dir = ProjectService::projects_dir();
        let slug = slugify_project_name("BrokenProject");
        let dir = projects_dir.join(&slug).join("skills").join("broken-skill");
        std::fs::create_dir_all(&dir).unwrap();
        // No frontmatter at all.
        std::fs::write(dir.join("SKILL.md"), "Just a body, no YAML.\n").unwrap();

        load_all_skills()
    })
    .await;

    let _ = std::fs::remove_dir_all(home_for_profile(Some(&profile)));

    // Should not contain the broken skill.
    let found = out.iter().find(|s| s.name == "broken-skill");
    assert!(found.is_none(), "broken skill should be silently skipped");
}
