//! Structural phantom tell: a runnable shell command presented as the answer
//! by a turn that never invoked a shell (#1194).
//!
//! Every other phantom detector matches PHRASING, so every one of them has a
//! next gap: the model can always word an announcement in a way no list
//! anticipates. #1192 and #1193 each closed one such gap, and neither would
//! have caught the other's shape. This detector reads the SHAPE of the answer
//! instead, so it carries no phrase list and holds in every language.
//!
//! The claim it makes is narrow and hard to argue with: if the turn's answer
//! is a fenced block tagged as a shell, containing a command whose program we
//! recognise, and no tool ran this turn, then the command was not run. The
//! caller supplies the zero-tool-call gate; a turn that genuinely ran tools
//! and then shows the command it ran is none of this module's business.

use super::phantom::KNOWN_PROGRAMS;

/// Fence info-strings that mean "the body of this block is a shell command".
///
/// Deliberately excludes `text`, `plain` and an empty tag. An untagged fence
/// is used for quoted output, file contents and error messages at least as
/// often as for commands, and treating it as a shell block would flag honest
/// answers that paste a log.
const SHELL_TAGS: &[&str] = &[
    "bash",
    "sh",
    "zsh",
    "shell",
    "shell-session",
    "sh-session",
    "console",
    "terminal",
    "fish",
    "ksh",
    "cmd",
    "powershell",
    "ps1",
];

/// Prompt markers that belong to a pasted transcript rather than the command.
const PROMPT_MARKERS: &[char] = &['$', '%', '>', '#'];

/// Shell keywords and wrappers that sit in FRONT of the real program.
///
/// `for n in 1 2; do gh api …; done` splits into a segment beginning `do`,
/// and the `gh` call behind it is the whole point of the block. Stripping a
/// leading run of these is safer than scanning every word of the segment,
/// which would read a path argument that happens to be named `git` as a
/// command.
const LEADING_KEYWORDS: &[&str] = &[
    "do", "then", "else", "elif", "sudo", "time", "nohup", "exec", "command", "nice", "builtin",
];

/// Does `text` present a runnable shell command inside a shell-tagged fence?
///
/// Only call this when the turn ran zero tools; the answer is meaningless
/// otherwise.
pub(crate) fn narrates_unrun_shell_block(text: &str) -> bool {
    shell_block_bodies(text)
        .iter()
        .any(|body| holds_known_command(body))
}

/// Bodies of every shell-tagged fenced block in `text`.
///
/// An UNTERMINATED block still counts. A turn cut off mid-block is not a
/// finished answer, and dropping it would make truncation a way through.
fn shell_block_bodies(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut open: Option<String> = None;
    for line in text.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("```") {
            match open.take() {
                // Closing fence for a shell block we were collecting.
                Some(body) => out.push(body),
                // Opening fence. A non-shell tag leaves `open` at None, so
                // this block's own closing fence is read as another opener
                // with an empty tag and is likewise ignored.
                None => {
                    let tag = rest
                        .split_whitespace()
                        .next()
                        .unwrap_or_default()
                        .to_lowercase();
                    if SHELL_TAGS.contains(&tag.as_str()) {
                        open = Some(String::new());
                    }
                }
            }
            continue;
        }
        if let Some(body) = open.as_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    if let Some(body) = open {
        out.push(body);
    }
    out
}

/// Does any line of a block body start a command we recognise?
fn holds_known_command(body: &str) -> bool {
    body.lines().any(starts_known_command)
}

/// Is `line` a shell command whose program is in the allowlist?
///
/// Split on the connectors first, so a command reached through a `cd` prefix
/// or a pipeline is found by its real program: `cd ~/repo && gh issue view 1`
/// is a `gh` call, not a `cd` call. Requires an argument as well as a
/// program, matching the inline-backtick rule, so a bare `ls` in prose is not
/// a command claim.
fn starts_known_command(line: &str) -> bool {
    let line = line.trim().trim_start_matches(PROMPT_MARKERS).trim();
    line.split(['|', ';', '\n'])
        .flat_map(|seg| seg.split("&&"))
        .flat_map(|seg| seg.split("||"))
        .any(segment_is_known_command)
}

/// Is one connector-delimited segment a call to a program we recognise?
fn segment_is_known_command(seg: &str) -> bool {
    let mut words = seg
        .split_whitespace()
        .skip_while(|w| LEADING_KEYWORDS.contains(w));
    words
        .next()
        .is_some_and(|prog| KNOWN_PROGRAMS.contains(&prog))
        && words.next().is_some()
}
