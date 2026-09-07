//! Code that can run during a turn must not write to the terminal directly.
//!
//! Two reasons, both load-bearing (#1413):
//!
//! 1. **The TUI owns the screen.** ratatui draws frames; a `println!` from
//!    inside a turn lands outside the frame, so the render is left with
//!    characters it did not draw and no way to know they are there.
//! 2. **Redaction is a writer-level guarantee.** The scrubber added in #1322
//!    wraps the tracing layers, so every event routed through `tracing` has
//!    its secrets removed before it reaches a writer. Direct printing never
//!    touches that, and the sites this guard covers interpolate paths and
//!    error `Display`s, which is exactly the shape that leaked a bot token.
//!
//! The CLI is deliberately exempt: `src/cli/` and the userbot login command
//! run with no TUI, and their output IS the interface. Routing the login QR
//! through `tracing` would send it to `io::sink` whenever the TUI is up.

use std::path::Path;

/// Directories whose code can execute inside a turn.
const TURN_PATHS: &[&str] = &["src/brain", "src/tui", "src/memory", "src/services"];

/// Reached only from the `channel userbot-login` CLI subcommand, where stdout
/// is the correct surface and the TUI is not running.
const CLI_ONLY: &[&str] = &["src/channels/telegram/userbot/login.rs"];

fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// A printing macro invocation, ignoring occurrences inside comments so the
/// guard's own explanatory prose does not trip it.
fn prints_directly(line: &str) -> bool {
    let code = line.split("//").next().unwrap_or("");
    code.contains("println!") || code.contains("eprintln!") || code.contains("print!(")
}

#[test]
fn turn_code_never_writes_to_the_terminal() {
    let mut files = Vec::new();
    for dir in TURN_PATHS {
        rust_files(Path::new(dir), &mut files);
    }
    assert!(!files.is_empty(), "found no sources to scan; paths moved?");

    let mut offenders = Vec::new();
    for f in files {
        let path = f.to_string_lossy().replace('\\', "/");
        if path.starts_with("src/tests/") || CLI_ONLY.iter().any(|c| path.ends_with(c)) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&f) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            if prints_directly(line) {
                offenders.push(format!("{path}:{}", i + 1));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "these run during a turn and must use tracing, not direct printing:\n  {}\n\
         Printing here writes outside the ratatui frame and skips the redacting writer.",
        offenders.join("\n  ")
    );
}

/// The exemption is deliberate, so it is pinned: if the login flow moves out
/// of the CLI, this test starts failing and the exemption gets re-examined
/// rather than silently covering a file that now runs under the TUI.
#[test]
fn the_cli_login_exemption_still_points_at_a_real_file() {
    for c in CLI_ONLY {
        assert!(
            Path::new(c).is_file(),
            "exempted path no longer exists: {c} — re-check whether the exemption is still earned"
        );
    }
}
