//! Every compaction site builds its continuation prompt through one path.
//!
//! There are five places a compaction can wake the agent (Regular, MidLoop,
//! Emergency, PostTool, Manual), and each used to call `build_continuation`
//! itself with the same four arguments. Anything that has to ride *every*
//! continuation then has to be added in five places and stays correct only
//! while nobody forgets one. The advisory skill-inventory stamp (#125) is the
//! first such rider: it is appended after the body, so a site still calling
//! `build_continuation` directly produces a perfectly valid prompt with the
//! stamp silently missing.
//!
//! That failure is invisible to the unit tests around the stamp, which drive
//! `append_skill_stamp` directly and would keep passing. This guard pins the
//! structural property they cannot: the loop reaches the builder exactly once,
//! through `continuation_prompt`, so a sixth compaction site cannot quietly
//! skip what the other five carry.

use std::path::Path;

const TOOL_LOOP: &str = "src/brain/agent/service/tool_loop.rs";

/// Occurrences in real code, ignoring line comments so this guard's own
/// explanatory prose (and the loop's) does not count as a call.
fn code_occurrences(text: &str, needle: &str) -> Vec<usize> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| line.split("//").next().unwrap_or("").contains(needle))
        .map(|(i, _)| i + 1)
        .collect()
}

#[test]
fn the_tool_loop_reaches_the_continuation_builder_exactly_once() {
    let text = std::fs::read_to_string(Path::new(TOOL_LOOP))
        .unwrap_or_else(|e| panic!("{TOOL_LOOP} must be readable ({e}); did the module move?"));

    let direct = code_occurrences(&text, "build_continuation(");

    assert_eq!(
        direct.len(),
        1,
        "{TOOL_LOOP} calls build_continuation on {} lines ({:?}), but the loop must reach it \
         only through continuation_prompt. A site calling it directly builds a valid prompt \
         with every rider missing, starting with the #125 skill stamp, and no unit test sees it.",
        direct.len(),
        direct
    );
}

#[test]
fn the_single_path_is_continuation_prompt_and_it_carries_the_stamp() {
    let text = std::fs::read_to_string(Path::new(TOOL_LOOP))
        .unwrap_or_else(|e| panic!("{TOOL_LOOP} must be readable ({e}); did the module move?"));

    assert!(
        !code_occurrences(&text, "async fn continuation_prompt(").is_empty(),
        "the shared construction path is gone from {TOOL_LOOP}; the guard above is \
         measuring nothing"
    );
    assert!(
        !code_occurrences(&text, "append_skill_stamp(").is_empty(),
        "the shared path no longer appends the skill stamp, so no compaction carries it"
    );
    assert!(
        !code_occurrences(&text, "active_skills_for_session(").is_empty(),
        "the stamp is no longer fed from the session's active-skill registry, so it \
         would render empty for every session"
    );
}
