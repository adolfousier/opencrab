//! #1194: a runnable shell command handed back as the answer by a turn that
//! ran no tools is phantom, on structure alone.
//!
//! The phrase detectors (#1192, #1193) each closed one wording gap and
//! neither would have caught the other's shape. This tell carries no phrase
//! list, so it does not have a next wording to miss.

use crate::brain::agent::service::fenced_command::narrates_unrun_shell_block;

/// The reported turn: one line of prose, then the command it never ran.
const REPORTED: &str = "Queue locked: everything except #933/#1112. That's ten \
     issues, fetching all specs fresh before planning:\n\n\
     ```bash\n\
     cd ~/srv/rs/opencrabs && for n in 1176 1181; do gh api repos/o/r/issues/$n; done\n\
     ```";

#[test]
fn test_reported_turn_is_caught_on_structure_alone() {
    assert!(
        narrates_unrun_shell_block(REPORTED),
        "#1194: a bash fence with a gh call is a command that did not run"
    );
}

#[test]
fn test_command_behind_a_cd_prefix_is_found_by_its_real_program() {
    // `cd` is not in the allowlist; the command must be found by `gh`.
    assert!(narrates_unrun_shell_block(
        "```bash\ncd /tmp && gh issue list --limit 5\n```"
    ));
    assert!(narrates_unrun_shell_block(
        "```sh\ncat log.txt | grep -c ERROR\n```"
    ));
    assert!(narrates_unrun_shell_block(
        "```console\n$ cargo test --all-features\n```"
    ));
}

#[test]
fn test_non_shell_fences_are_untouched() {
    // A code answer is an answer. Only shell-tagged fences are commands.
    for block in [
        "```rust\nlet out = git_status();\n```",
        "```python\nsubprocess.run([\"git\", \"status\"])\n```",
        "```json\n{\"cmd\": \"git status --short\"}\n```",
        "```\ngit status --short\n```",
        "```toml\ncommand = \"cargo build\"\n```",
    ] {
        assert!(
            !narrates_unrun_shell_block(block),
            "#1194: non-shell fence flagged: {block:?}"
        );
    }
}

#[test]
fn test_shell_fence_without_a_known_program_is_untouched() {
    // Pasted output, an error message, or a command we do not recognise.
    for block in [
        "```bash\nerror: could not compile `opencrabs`\n```",
        "```bash\nexport RUST_LOG=debug\n```",
        "```console\nhello world\n```",
    ] {
        assert!(
            !narrates_unrun_shell_block(block),
            "#1194: shell fence with no known command flagged: {block:?}"
        );
    }
}

#[test]
fn test_bare_program_without_arguments_is_not_a_command_claim() {
    // Same rule as the inline-backtick check: program AND an argument.
    assert!(!narrates_unrun_shell_block("```bash\nls\n```"));
    assert!(narrates_unrun_shell_block("```bash\nls -la /tmp\n```"));
}

#[test]
fn test_unterminated_block_still_counts() {
    // A turn cut off mid-block is not a finished answer; dropping it would
    // make truncation a way through.
    assert!(narrates_unrun_shell_block(
        "Here is the plan:\n\n```bash\ngit log --oneline -20"
    ));
}

#[test]
fn test_prose_alone_is_never_flagged() {
    assert!(!narrates_unrun_shell_block(
        "I ran git status and the tree is clean."
    ));
    assert!(!narrates_unrun_shell_block(""));
}

#[test]
fn test_shell_keywords_do_not_hide_the_program() {
    // `for … do gh api …; done` splits into a segment starting with `do`.
    // The call behind the keyword is the whole point of the block.
    for block in [
        "```bash\nfor n in 1 2; do gh api repos/o/r/issues/$n; done\n```",
        "```bash\nif [ -f x ]; then cat x --number; fi\n```",
        "```bash\nsudo systemctl restart opencrabs\n```",
    ] {
        assert!(
            narrates_unrun_shell_block(block),
            "#1194: keyword hid the program: {block:?}"
        );
    }
}

#[test]
fn test_keyword_stripping_does_not_invent_commands() {
    // Stripping a leading keyword must not turn an argument into a program.
    assert!(!narrates_unrun_shell_block(
        "```bash\nthen else do done\n```"
    ));
    assert!(!narrates_unrun_shell_block(
        "```bash\necho \"run git status\" > notes.txt\n```"
    ));
}
