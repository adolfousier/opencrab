//! Same-character run detection for a streamed reasoning window (#1351).
//!
//! GLM-5.3-Flash has a known server-side degeneration where `reasoning_content`
//! turns into `!!!!!` and keeps going. The substring guard beside this one
//! (`detect_text_repetition`) needs two matching 300-byte halves, so it sees a
//! pure run only once it is 600 bytes long and, when it fires, names it a
//! repetition rather than what it is. This check fires at 64 and says
//! "same-character run", so the log reads correctly in a postmortem.
//!
//! Only the run ending at the window's tail is inspected: deltas append, so a
//! run that reaches the threshold is the trailing run on the delta that pushed
//! it there. That keeps the check O(run) per delta instead of O(window).

/// Runs of one character the model must not be blamed for: whitespace (code
/// indentation, blank lines) is never a run, and the characters used for
/// rules and dividers (`-----`, `=====`, `*****`) need four times the
/// threshold before they count, since an 80-column divider in reasoning is
/// legitimate.
const DIVIDER_CHARS: [char; 9] = ['-', '=', '_', '*', '#', '~', '.', '+', '|'];

/// Below this length nothing is a run. 64 is longer than any punctuation a
/// model writes on purpose and far shorter than the 600 bytes the substring
/// guard needs.
pub(crate) const MIN_RUN: usize = 64;

/// The character and length of the run ending at the tail of `window`, when
/// that run is long enough to be degeneration rather than punctuation.
pub(crate) fn degenerate_run(window: &str, min_run: usize) -> Option<(char, usize)> {
    let last = window.chars().next_back()?;
    if last.is_whitespace() {
        return None;
    }
    let len = window.chars().rev().take_while(|c| *c == last).count();
    let needed = if DIVIDER_CHARS.contains(&last) {
        min_run * 4
    } else {
        min_run
    };
    (len >= needed).then_some((last, len))
}
