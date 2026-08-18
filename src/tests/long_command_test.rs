//! Which commands detach (#722) and, since #1093, which only look like they
//! should. A marker in a heredoc body or a quoted argument is data: detaching
//! on it ends the turn on a command that takes milliseconds, and every
//! completion comes back as an injected message that starts another turn.

use crate::utils::long_command::{Detach, classify, is_known_long};

#[test]
fn known_long_matches_the_named_cases() {
    assert!(is_known_long("cargo test --all-features"));
    assert!(is_known_long("cd ~/proj && cargo build --release"));
    assert!(is_known_long("npx remotion render Main out/x.mp4"));
    assert!(is_known_long("gh run watch 12345"));
    // Ordinary quick commands are not backgrounded.
    assert!(!is_known_long("ls -la"));
    assert!(!is_known_long("git status"));
    assert!(!is_known_long("cat README.md"));
}

#[test]
fn markers_in_command_position_still_background() {
    assert!(is_known_long("cargo test --all-features --lib plan_tool"));
    assert!(is_known_long("cargo fmt; echo done; cargo clippy --locked"));
    assert!(is_known_long("W=$(cargo clippy --all-features 2>&1)"));
    assert!(is_known_long("cargo fmt\ncargo test --lib"));
    assert!(is_known_long("for f in a b; do cargo test --lib; done"));
    assert!(is_known_long("time cargo build --release"));
}

#[test]
fn a_marker_that_is_only_mentioned_runs_inline() {
    // Heredoc body: the report names the command, the shell never runs it.
    assert!(!is_known_long(
        "cat > /tmp/note.md <<'EOF'\nRun cargo test --all-features to verify.\nEOF"
    ));
    // A marker alone on a body line must not read as a command position.
    assert!(!is_known_long(
        "cat >> src/tests/x.rs <<'RUST'\ncargo test --lib\nRUST"
    ));
    // Quoted grep pattern.
    assert!(!is_known_long(
        "grep -c -i \"cargo test\\|cargo clippy\" src/tests/x.rs"
    ));
    // Interpreter heredoc whose body inserts the phrase into a source file.
    assert!(!is_known_long(
        "python3 - <<'PY'\ns = \"cargo clippy --all-features\"\nPY"
    ));
    // An unquoted heredoc body is still a body.
    assert!(!is_known_long("cat > x <<EOF\ncargo build --release\nEOF"));
    // Echoed text, quoted.
    assert!(!is_known_long("echo \"cargo build --release\""));
}

#[test]
fn a_here_string_is_not_a_heredoc() {
    // `<<<` feeds a word to stdin; nothing after it opens a body, so a marker
    // on a later line is ordinary shell code again.
    assert!(is_known_long("cat <<< hello\ncargo test --lib"));
}

#[test]
fn classify_reports_why_a_command_ran_inline() {
    assert_eq!(
        classify("cargo test --lib"),
        Detach::Yes {
            marker: "cargo test"
        }
    );
    assert_eq!(
        classify("echo \"cargo test --lib\""),
        Detach::Mentioned {
            marker: "cargo test"
        }
    );
    assert_eq!(classify("git status"), Detach::No);
}
