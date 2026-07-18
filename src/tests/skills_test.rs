//! Tests for the skill loader — frontmatter parsing, built-in registry,
//! and user-directory overlay.

use crate::brain::skills::{Skill, SkillSource, load_all_skills, resolve_skill};

#[test]
fn parses_minimal_frontmatter() {
    let raw = "---\nname: foo\ndescription: A test skill\n---\n\nBody here.\n";
    let skill = Skill::parse("foo", raw, SkillSource::Builtin).unwrap();
    assert_eq!(skill.name, "foo");
    assert_eq!(skill.description, "A test skill");
    assert_eq!(skill.body, "Body here.");
    assert_eq!(skill.source, SkillSource::Builtin);
}

#[test]
fn parses_crlf_line_endings() {
    let raw = "---\r\nname: foo\r\ndescription: windows\r\n---\r\n\r\nBody.\r\n";
    let skill = Skill::parse("foo", raw, SkillSource::Builtin).unwrap();
    assert_eq!(skill.description, "windows");
    assert_eq!(skill.body, "Body.");
}

#[test]
fn tolerates_utf8_bom() {
    let raw = "\u{FEFF}---\nname: foo\ndescription: bom\n---\n\nBody.\n";
    let skill = Skill::parse("foo", raw, SkillSource::Builtin).unwrap();
    assert_eq!(skill.description, "bom");
}

#[test]
fn strips_quotes_around_values() {
    let raw_double = "---\nname: foo\ndescription: \"quoted\"\n---\nBody.\n";
    let skill = Skill::parse("foo", raw_double, SkillSource::Builtin).unwrap();
    assert_eq!(skill.description, "quoted");

    let raw_single = "---\nname: foo\ndescription: 'single'\n---\nBody.\n";
    let skill = Skill::parse("foo", raw_single, SkillSource::Builtin).unwrap();
    assert_eq!(skill.description, "single");
}

#[test]
fn name_falls_back_to_argument_when_frontmatter_lacks_it() {
    // Frontmatter omits `name:` — the directory name is authoritative.
    let raw = "---\ndescription: no name field\n---\nBody.\n";
    let skill = Skill::parse("from-dir", raw, SkillSource::User).unwrap();
    assert_eq!(skill.name, "from-dir");
}

#[test]
fn missing_description_is_an_error() {
    let raw = "---\nname: foo\n---\nBody.\n";
    let err = Skill::parse("foo", raw, SkillSource::Builtin).unwrap_err();
    assert!(err.contains("description"), "got: {err}");
}

#[test]
fn missing_frontmatter_fence_is_an_error() {
    let raw = "Just a body, no fence.";
    let err = Skill::parse("foo", raw, SkillSource::Builtin).unwrap_err();
    assert!(err.contains("frontmatter"), "got: {err}");
}

#[test]
fn unmatched_open_fence_is_an_error() {
    // Opens with --- but never closes — should not silently swallow the body.
    let raw = "---\nname: foo\ndescription: leak\n";
    let err = Skill::parse("foo", raw, SkillSource::Builtin).unwrap_err();
    assert!(err.contains("frontmatter"), "got: {err}");
}

#[test]
fn unknown_keys_are_ignored_for_forward_compat() {
    let raw = "---\nname: foo\ndescription: ok\nmodel: claude-haiku-4-5\nfuture_field: whatever\n---\nBody.\n";
    let skill = Skill::parse("foo", raw, SkillSource::Builtin).unwrap();
    assert_eq!(skill.description, "ok");
}

#[test]
fn body_preserves_internal_blank_lines_and_markdown() {
    let raw =
        "---\nname: foo\ndescription: ok\n---\n\n# Heading\n\nParagraph 1.\n\n- item 1\n- item 2\n";
    let skill = Skill::parse("foo", raw, SkillSource::Builtin).unwrap();
    assert!(skill.body.starts_with("# Heading"));
    assert!(skill.body.contains("- item 1\n- item 2"));
}

#[test]
fn builtin_security_audit_loads_via_resolver() {
    // The compile-time embedded security-audit skill must always resolve.
    let skill = resolve_skill("security-audit").expect("built-in 'security-audit' must exist");
    assert_eq!(skill.source, SkillSource::Builtin);
    assert!(
        skill.description.to_lowercase().contains("security"),
        "description should mention security"
    );
    assert!(!skill.body.is_empty());
}

#[test]
fn builtin_cost_estimate_loads_via_resolver() {
    let skill = resolve_skill("cost-estimate").expect("built-in 'cost-estimate' must exist");
    assert_eq!(skill.source, SkillSource::Builtin);
    assert!(skill.description.to_lowercase().contains("cost"));
}

#[test]
fn unknown_skill_returns_none() {
    assert!(resolve_skill("does-not-exist-anywhere").is_none());
}

#[test]
fn parses_review_gate_flag_and_prepends_reminder() {
    // A flagged skill's slash prompt carries the review gate: output first,
    // side effects only after the user's explicit approval, yolo included.
    let raw =
        "---\nname: gated\ndescription: Gated skill\nreview_gate: true\n---\n\nDraft the thing.\n";
    let skill = Skill::parse("gated", raw, SkillSource::User).unwrap();
    assert!(skill.review_gate);
    let prompt = skill.prompt_body();
    assert!(
        prompt.contains("SKILL REVIEW GATE"),
        "reminder missing: {prompt}"
    );
    assert!(
        prompt.contains("Draft the thing."),
        "body must follow the reminder: {prompt}"
    );

    // Accepted truthy spellings.
    for spelling in ["true", "yes", "1", "on", "TRUE"] {
        let raw = format!("---\nname: g\ndescription: d\nreview_gate: {spelling}\n---\n\nBody.\n");
        let skill = Skill::parse("g", &raw, SkillSource::User).unwrap();
        assert!(
            skill.review_gate,
            "spelling '{spelling}' must parse as true"
        );
    }
}

#[test]
fn review_gate_defaults_false_and_prompt_is_untouched() {
    // Unflagged skills dispatch their body verbatim — exactly as before.
    let raw = "---\nname: plain\ndescription: Plain skill\n---\n\nBody.\n";
    let skill = Skill::parse("plain", raw, SkillSource::User).unwrap();
    assert!(!skill.review_gate);
    assert_eq!(skill.prompt_body(), "Body.");

    // False-ish and unknown values stay false.
    for spelling in ["false", "no", "0", "off", "maybe"] {
        let raw = format!("---\nname: p\ndescription: d\nreview_gate: {spelling}\n---\n\nBody.\n");
        let skill = Skill::parse("p", &raw, SkillSource::User).unwrap();
        assert!(
            !skill.review_gate,
            "spelling '{spelling}' must parse as false"
        );
        assert_eq!(skill.prompt_body(), "Body.");
    }
}

#[test]
fn load_all_includes_every_builtin() {
    let skills = load_all_skills();
    let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"security-audit"), "names: {:?}", names);
    assert!(names.contains(&"cost-estimate"), "names: {:?}", names);
}

/// Issue #231: a skill placed in a NAMED profile's own skills dir
/// (`~/.opencrabs/profiles/<name>/skills/<skill>/SKILL.md`) is discovered when
/// that profile is active, and is NOT visible from a different profile.
///
/// This pins the actual contract behind the report: profile skills resolve
/// through the profile-aware `opencrabs_home()`, so they load under the profile
/// where they are declared (`-p <name>`) — both at startup and on the live
/// reload path. They are deliberately invisible from other profiles, because a
/// profile is an isolation boundary, not a shared discovery tier. A global
/// scan of `profiles/*/skills/` would leak every profile's skills into every
/// other profile and is exactly what this test guards against.
#[tokio::test]
async fn profile_skill_is_discovered_under_its_own_profile_only() {
    use crate::config::profile::{home_for_profile, with_profile_home_async};

    let declared = format!("skill-decl-{}", uuid::Uuid::new_v4());
    let other = format!("skill-other-{}", uuid::Uuid::new_v4());

    // Declare a skill inside the `declared` profile's own skills directory.
    with_profile_home_async(Some(&declared), async {
        let dir = crate::config::opencrabs_home()
            .join("skills")
            .join("profile-only-skill");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            "---\nname: profile-only-skill\ndescription: Lives in one profile\n---\n\nBody.\n",
        )
        .unwrap();
    })
    .await;

    // Visible while that profile is active.
    let seen_here = with_profile_home_async(Some(&declared), async {
        load_all_skills()
            .iter()
            .any(|s| s.name == "profile-only-skill")
    })
    .await;

    // Invisible from a different profile (isolation holds).
    let seen_elsewhere = with_profile_home_async(Some(&other), async {
        load_all_skills()
            .iter()
            .any(|s| s.name == "profile-only-skill")
    })
    .await;

    let _ = std::fs::remove_dir_all(home_for_profile(Some(&declared)));
    let _ = std::fs::remove_dir_all(home_for_profile(Some(&other)));

    assert!(
        seen_here,
        "profile-declared skill must load under its own profile"
    );
    assert!(
        !seen_elsewhere,
        "profile-declared skill must NOT leak into a different profile"
    );
}
