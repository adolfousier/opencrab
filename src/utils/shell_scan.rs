//! One quote-aware pass over a shell command, shared by everything that has to
//! tell shell syntax from data (#722, #790).
//!
//! Two independent scanners had grown here. The label builder tracked quotes so
//! it would not cut `echo "a; b"` in half and invent a segment, and the
//! long-command classifier blanked quoted spans so a marker inside a `grep`
//! pattern could not read as a command. Same state machine twice, and only one
//! of the copies understood backslash escapes. Kept once so they cannot drift.

/// One character of a command, tagged with what the shell would make of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellChar {
    /// The character itself, never dropped, so a scan can be reassembled.
    pub ch: char,
    /// Data rather than syntax: inside quotes, or backslash-escaped. A
    /// separator that is literal separates nothing.
    pub literal: bool,
    /// A quote character opening or closing a span. Syntax, but it carries no
    /// meaning of its own, so a caller looking for commands skips it.
    pub quote_mark: bool,
}

/// Tag every character of `command` as syntax, quote mark, or literal data.
///
/// Single quotes protect everything including backslashes; double quotes
/// protect everything except a backslash escape, matching POSIX closely enough
/// for the two decisions taken on the result (where to split, what to ignore).
/// An unterminated quote simply runs to the end of the input, which is the
/// conservative reading: the tail counts as data.
pub fn scan(command: &str) -> Vec<ShellChar> {
    let mut out = Vec::with_capacity(command.len());
    let mut chars = command.chars();
    let mut quote: Option<char> = None;

    while let Some(ch) = chars.next() {
        let escapes = match quote {
            None => ch == '\\',
            // Inside single quotes a backslash is an ordinary character.
            Some(q) => ch == '\\' && q == '"',
        };
        if escapes {
            out.push(ShellChar {
                ch,
                literal: true,
                quote_mark: false,
            });
            if let Some(next) = chars.next() {
                out.push(ShellChar {
                    ch: next,
                    literal: true,
                    quote_mark: false,
                });
            }
            continue;
        }

        match quote {
            None if ch == '\'' || ch == '"' => {
                quote = Some(ch);
                out.push(ShellChar {
                    ch,
                    literal: false,
                    quote_mark: true,
                });
            }
            None => out.push(ShellChar {
                ch,
                literal: false,
                quote_mark: false,
            }),
            Some(q) if ch == q => {
                quote = None;
                out.push(ShellChar {
                    ch,
                    literal: false,
                    quote_mark: true,
                });
            }
            Some(_) => out.push(ShellChar {
                ch,
                literal: true,
                quote_mark: false,
            }),
        }
    }
    out
}

/// Replace every quoted span, its quote marks, and every escaped character with
/// a space, leaving only the characters the shell would read as syntax.
///
/// Positions outside quotes are preserved, so a caller can still ask what
/// precedes a match.
pub fn blank_quoted(command: &str) -> String {
    scan(command)
        .into_iter()
        .map(|c| if c.literal || c.quote_mark { ' ' } else { c.ch })
        .collect()
}
