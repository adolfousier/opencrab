//! Reading a failed `bash` call: which population it belongs to, what it
//! exited with, and what it actually said (#1068).
//!
//! `bash` carries two very different failure populations under one tool name.
//! A command the model got wrong (bad syntax, an invented binary, a bare REPL)
//! is agent behaviour the RSI cycle is supposed to see and correct. A
//! well-formed command that hit a missing file, a closed port or a dead
//! service says nothing about the tool or the agent, and counting it as a
//! defect made `tool_failure|bash` the ledger's loudest signal, steering RSI
//! at "bash is broken" when the finding was neither.
//!
//! Kept separate from the policy that consumes it so the classification can be
//! exercised on real snippet shapes without going through the ledger.

/// Which population a failed bash call belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BashFailureKind {
    /// The command was wrong. Agent behaviour, worth learning from.
    ModelError,
    /// The command was fine, the environment was not. Not a defect.
    Environmental,
    /// Neither marker set matched. Treated as a genuine failure, because
    /// guessing "environmental" here would quietly drop real defects out of
    /// the success-rate denominator.
    Unknown,
}

/// Shell-error markers that mean the command itself was wrong.
///
/// Checked first and given precedence: a script that dies on a syntax error
/// after touching a missing path would otherwise be laundered into
/// "environmental" by the path message and disappear from RSI's view.
const MODEL_ERROR_MARKERS: &[&str] = &[
    "syntax error",
    "unexpected token",
    "unexpected end of file",
    "parse error",
    "unbound variable",
    "bad substitution",
    "command not found",
    "no such command",
    "invalid option",
    "illegal option",
    "unknown option",
    "unrecognized option",
    "missing operand",
    "too many arguments",
];

/// Markers for a well-formed command meeting a hostile environment.
///
/// Deliberately specific. The obvious shorthand `"not found"` would swallow
/// `command not found`, which is the single most common model error in the
/// ledger; `#236`'s regression test has pinned that string as a genuine defect
/// since before this classifier existed.
const ENVIRONMENTAL_MARKERS: &[&str] = &[
    "no such file or directory",
    "cannot access",
    "does not exist",
    "permission denied",
    "operation not permitted",
    "read-only file system",
    "no space left on device",
    "resource temporarily unavailable",
    "device or resource busy",
    "connection refused",
    "connection reset",
    "connection timed out",
    "network is unreachable",
    "host is unreachable",
    "no route to host",
    "could not resolve host",
    "name or service not known",
    "temporary failure in name resolution",
    "address already in use",
    "timed out",
    "timeout",
    "broken pipe",
    "could not connect",
    "service unavailable",
    "is not running",
    "communications error",
];

/// Which population this failure belongs to.
///
/// Scans stderr alone when the snippet carries a stderr section, and the whole
/// snippet otherwise. Captured stdout is excluded on purpose: a `cat` of a log
/// or an `ls` of a directory can contain any of these phrases as ordinary
/// content, and a match there would exempt a real defect from the success
/// rate on the strength of a filename.
pub fn classify(snippet: &str) -> BashFailureKind {
    let scanned = stderr_section(snippet)
        .unwrap_or(snippet)
        .to_ascii_lowercase();
    if MODEL_ERROR_MARKERS.iter().any(|m| scanned.contains(m)) {
        return BashFailureKind::ModelError;
    }
    if ENVIRONMENTAL_MARKERS.iter().any(|m| scanned.contains(m)) {
        return BashFailureKind::Environmental;
    }
    BashFailureKind::Unknown
}

/// The exit status the bash tool reported, when the snippet still carries it.
///
/// The tool's error line leads the snippet as `Command exited with code N`.
/// Parsed rather than plumbed through as a parameter because five call sites
/// hand the ledger nothing but this string.
pub fn exit_code(snippet: &str) -> Option<i32> {
    let rest = snippet.split_once("exited with code ")?.1;
    let digits: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    digits.parse().ok()
}

/// The head of the command's stderr, trimmed, or `None` when it wrote none.
///
/// The diagnostic line is normally the first thing on stderr, and it is what a
/// human reads first when triaging one of these rows.
pub fn stderr_head(snippet: &str, cap: usize) -> Option<&str> {
    let stderr = stderr_section(snippet)?.trim();
    if stderr.is_empty() {
        return None;
    }
    Some(match stderr.char_indices().nth(cap) {
        Some((end, _)) => &stderr[..end],
        None => stderr,
    })
}

/// The stderr the bash tool appended, if the snippet has a stderr section.
fn stderr_section(snippet: &str) -> Option<&str> {
    snippet.split_once("STDERR:\n").map(|(_, tail)| tail)
}
