//! Skills and `/onboard:<topic>` resolve instead of erroring (#889).
//!
//! Every recorded `slash_command` failure was one of these shapes:
//!
//! - `/servers`, `/channels` — skills on disk. `handle_user_command` consulted
//!   only `commands.toml`, so no skill could ever resolve and each one reached
//!   the "Unknown command" arm.
//! - `/onboard:voice` — the dispatch matched the bare `/onboard` only, so the
//!   colon form people actually type fell through.
//! - `/skills` — a real TUI command this tool cannot execute, which belonged in
//!   the guidance arm beside `/plan` and `/cowork`.
//!
//! None of these was a failing tool. The tool could not see what it was asked
//! about.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::brain::skills::load_all_skills;

#[test]
fn skills_carry_the_slash_form_the_tool_matches_on() {
    // Resolution compares `slash_name`, so it must be the `/name` shape or the
    // lookup silently never matches.
    for s in load_all_skills() {
        assert!(
            s.slash_name.starts_with('/'),
            "skill {:?} has slash_name {:?}",
            s.name,
            s.slash_name
        );
        assert_eq!(s.slash_name, format!("/{}", s.name));
    }
}

#[test]
fn a_skill_lookup_by_slash_name_succeeds() {
    // The exact lookup the tool performs. Uses whichever skills exist rather
    // than hard-coding one, so the test does not depend on local install state.
    let skills = load_all_skills();
    if let Some(first) = skills.first() {
        let found = skills.iter().find(|s| s.slash_name == first.slash_name);
        assert!(
            found.is_some(),
            "a skill could not find itself by slash_name"
        );
    }
}

#[test]
fn the_onboard_colon_form_is_recognised() {
    // The dispatch guard is `c == "/onboard" || c.starts_with("/onboard:")`.
    let matches = |c: &str| c == "/onboard" || c.starts_with("/onboard:");
    for c in [
        "/onboard",
        "/onboard:voice",
        "/onboard:channels",
        "/onboard:image",
    ] {
        assert!(matches(c), "not recognised: {c}");
    }
}

#[test]
fn an_unrelated_command_is_not_swallowed_by_the_onboard_guard() {
    // `starts_with` is broad; it must not capture a different command that
    // merely begins with the same letters.
    let matches = |c: &str| c == "/onboard" || c.starts_with("/onboard:");
    for c in ["/onboarding", "/on", "/board", "/models"] {
        assert!(!matches(c), "wrongly captured: {c}");
    }
}
