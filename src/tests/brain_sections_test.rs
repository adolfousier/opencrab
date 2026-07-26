//! Reading part of a brain file instead of all of it (#800).
//!
//! `load_brain_file` was all-or-nothing, so consulting MEMORY.md meant loading
//! the whole thing. The file is append-only and only grows, so the cost of
//! reading rose exactly as it accumulated more worth reading. Not calling it
//! was locally rational, which is why it was written constantly and read
//! almost never.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::brain::brain_sections::{find_sections, split_sections};

const FILE: &str = "\
# Memory

Preamble line.

## Telegram owner gate

Only the owner sees channel commands. Non-owners get an ephemeral rejection.

## Build workflow

Use clippy with all features. Never plain check.

## Release workflow

Tag builds run on five platforms.
";

#[test]
fn a_query_returns_only_the_matching_section() {
    let m = find_sections(FILE, "telegram owner gate");
    assert_eq!(m.sections.len(), 1, "got: {:?}", m.sections);
    assert!(m.sections[0].heading.contains("Telegram owner gate"));
    assert!(m.sections[0].body.contains("ephemeral rejection"));
}

#[test]
fn a_matching_section_comes_back_whole() {
    // The load-bearing property: half a rule reads as complete while dropping
    // the qualifier that made it correct.
    let m = find_sections(FILE, "clippy");
    let rendered = m.sections[0].render();
    assert!(rendered.contains("## Build workflow"), "{rendered}");
    assert!(
        rendered.contains("Use clippy with all features."),
        "{rendered}"
    );
    assert!(rendered.contains("Never plain check."), "{rendered}");
}

#[test]
fn unrelated_sections_are_not_returned() {
    let m = find_sections(FILE, "clippy");
    assert!(
        m.sections.iter().all(|s| !s.heading.contains("Telegram")),
        "unrelated section leaked: {:?}",
        m.sections
    );
}

#[test]
fn a_section_covering_more_terms_ranks_first() {
    // Scored by DISTINCT terms matched, so a section answering the whole
    // question beats one repeating a single common word.
    let m = find_sections(FILE, "release workflow platforms");
    assert!(
        m.sections[0].heading.contains("Release workflow"),
        "got: {:?}",
        m.sections[0]
    );
}

#[test]
fn no_match_says_so_rather_than_returning_nothing() {
    // Silence would read as "no such rule exists" when the file may simply
    // use different words.
    let m = find_sections(FILE, "kubernetes helm chart");
    assert!(m.sections.is_empty());
    let rendered = m.render("MEMORY.md", "kubernetes helm chart");
    assert!(rendered.contains("No section"), "{rendered}");
    assert!(
        rendered.contains("without a query"),
        "must offer the full read as a way out: {rendered}"
    );
}

#[test]
fn omissions_are_stated_never_silent() {
    // A capped result that reads as complete is how a partial read becomes a
    // wrong answer.
    let mut big = String::new();
    for i in 0..12 {
        big.push_str(&format!("## Rule {i} workflow\n\nWorkflow detail {i}.\n\n"));
    }
    let m = find_sections(&big, "workflow");
    assert!(m.omitted > 0, "expected caps to bite: {m:?}");
    let rendered = m.render("MEMORY.md", "workflow");
    assert!(rendered.contains("omitted"), "{rendered}");
}

#[test]
fn a_heading_inside_a_fence_does_not_split_a_rule() {
    // `# comment` in a shell block is not a heading; treating it as one would
    // cut the rule in half.
    let text = "## Build\n\nRun this:\n\n```sh\n# always all features\ncargo clippy --all-features\n```\n\nNothing else.\n";
    let sections = split_sections(text);
    assert_eq!(sections.len(), 1, "got: {sections:?}");
    assert!(sections[0].body.contains("Nothing else."));
}

#[test]
fn an_empty_query_matches_nothing() {
    // The caller treats an absent query as "load it all"; this must not
    // silently return the file section by section.
    assert!(find_sections(FILE, "   ").sections.is_empty());
}

#[test]
fn very_short_terms_are_ignored() {
    // Two-letter noise would match nearly every section and defeat the point.
    assert!(find_sections(FILE, "on to a").sections.is_empty());
}

#[test]
fn a_file_with_no_headings_is_still_searchable() {
    let text = "Just a flat note about the release workflow and nothing else.";
    let m = find_sections(text, "release workflow");
    assert_eq!(m.sections.len(), 1);
    assert!(m.sections[0].heading.is_empty());
}
