//! Turn a shell command into a label a human can read at a glance (#790, #791).
//!
//! Every long-running command the agent issues starts by entering the working
//! directory, so a label that keeps the `cd` keeps the one part that is
//! identical for every command and drops the part that identifies it. At the 28
//! characters the input-border indicator allows, that leaves a field showing
//! nothing but a path.
//!
//! Multi-line commands carry a second problem: a newline reaching a single-line
//! renderer is not drawn, so the tail of one line touches the head of the next
//! and produces a token that was never in the command (`opencrabs` + `echo` ->
//! `opencrabsecho`). That reads as a real command and is not one, which is
//! worse than truncation.
//!
//! Both are fixed here, once, so the TUI rows, the border indicator and the
//! channel summaries cannot drift apart.

/// A single-line, human-readable label for `command`.
///
/// Leading `cd <path>` segments are dropped, every remaining segment is kept,
/// and segments are joined with `"; "` so nothing can fuse across a break.
/// Returns an empty string for an empty command; callers supply their own
/// placeholder.
pub fn command_label(command: &str) -> String {
    let segments = split_segments(command);
    if segments.is_empty() {
        return String::new();
    }

    // Strip only LEADING `cd`s. A `cd` in the middle or at the end is doing
    // something the reader needs to see, and dropping it would misdescribe the
    // command. Taking only the last segment (the previous behaviour) silently
    // discarded real work: `cd /x && cargo build && cargo test` labelled as
    // `cargo test` alone.
    let mut kept: Vec<&String> = segments.iter().skip_while(|s| is_bare_cd(s)).collect();

    // A command that is nothing but `cd` still has to render as something.
    if kept.is_empty() {
        kept = segments.iter().collect();
    }

    kept.iter()
        .map(|s| collapse_whitespace(s))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Is this segment a bare directory change and nothing else?
///
/// Segments are already split on every separator, so everything after `cd ` is
/// the path, spaces and quotes included.
fn is_bare_cd(segment: &str) -> bool {
    segment
        .strip_prefix("cd ")
        .is_some_and(|rest| !rest.trim().is_empty())
}

/// Split on top-level `\n`, `;` and `&&`, ignoring separators inside quotes.
///
/// Quote tracking is what makes this safe to run on arbitrary commands: a
/// naive `split(';')` would cut `echo "a; b"` in half and invent a segment.
fn split_segments(command: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut chars = command.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\'' if !in_double => {
                in_single = !in_single;
                current.push(c);
            }
            '"' if !in_single => {
                in_double = !in_double;
                current.push(c);
            }
            '\n' | ';' if !in_single && !in_double => {
                segments.push(std::mem::take(&mut current));
            }
            '&' if !in_single && !in_double && chars.peek() == Some(&'&') => {
                chars.next();
                segments.push(std::mem::take(&mut current));
            }
            _ => current.push(c),
        }
    }
    segments.push(current);

    segments
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Collapse runs of whitespace so a command formatted across columns reads as
/// one line. Any newline inside a segment is impossible (they are separators),
/// so this only tidies runs of spaces and tabs.
fn collapse_whitespace(segment: &str) -> String {
    segment.split_whitespace().collect::<Vec<_>>().join(" ")
}
