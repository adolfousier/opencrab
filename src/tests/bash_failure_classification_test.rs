//! Reading a failed bash call (#1068).
//!
//! `bash` carries two failure populations under one tool name. A command the
//! model got wrong is agent behaviour RSI should act on; a well-formed command
//! that met a missing file or a dead service is not a defect at all. Before
//! this split every one of them landed as `tool_failure|bash`, which made bash
//! the ledger's loudest failure source and pointed RSI at the wrong thing.
//!
//! Fixtures reproduce the snippet shape the ledger actually receives:
//! `build_tool_result_content` leads with the bash tool's error line, then
//! appends the captured stdout/stderr block.

use crate::brain::bash_failure::{BashFailureKind, classify, exit_code, stderr_head};

fn snippet(code: i32, stdout: &str, stderr: &str) -> String {
    let mut captured = String::new();
    if !stdout.is_empty() {
        captured.push_str(&format!("STDOUT:\n{stdout}"));
    }
    if !stderr.is_empty() {
        if !captured.is_empty() {
            captured.push_str("\n\n");
        }
        captured.push_str(&format!("STDERR:\n{stderr}"));
    }
    format!("Command exited with code {code}\n\n-- output captured before error --\n{captured}")
}

#[test]
fn a_missing_file_is_environmental() {
    let s = snippet(1, "", "cat: /etc/nope.conf: No such file or directory\n");
    assert_eq!(classify(&s), BashFailureKind::Environmental);
}

#[test]
fn a_dead_service_is_environmental() {
    for stderr in [
        "curl: (7) Failed to connect to localhost port 8931: Connection refused\n",
        "ssh: connect to host 10.0.0.9 port 22: Operation timed out\n",
        "curl: (6) Could not resolve host: example.invalid\n",
        "psql: error: connection to server failed: Permission denied\n",
    ] {
        assert_eq!(
            classify(&snippet(1, "", stderr)),
            BashFailureKind::Environmental,
            "must be environmental: {stderr}"
        );
    }
}

#[test]
fn a_command_the_model_got_wrong_stays_a_defect() {
    for stderr in [
        "bash: line 3: syntax error near unexpected token `fi'\n",
        "bash: frobnicate: command not found\n",
        "bash: line 1: MY_VAR: unbound variable\n",
        "ls: illegal option -- Z\n",
    ] {
        assert_eq!(
            classify(&snippet(2, "", stderr)),
            BashFailureKind::ModelError,
            "must stay a defect: {stderr}"
        );
    }
}

#[test]
fn a_model_error_wins_over_an_environmental_phrase() {
    // A script that touches a missing path and then dies on bad syntax would
    // otherwise be laundered into "environmental" by the first message and
    // disappear from RSI's view entirely.
    let s = snippet(
        2,
        "",
        "cat: /tmp/nope: No such file or directory\nbash: syntax error near unexpected token `done'\n",
    );
    assert_eq!(classify(&s), BashFailureKind::ModelError);
}

#[test]
fn an_unrecognised_failure_stays_a_defect() {
    // Guessing "environmental" on an unmatched snippet would quietly drop real
    // defects out of the success-rate denominator, which is the failure mode
    // #236 already paid for once.
    let s = snippet(1, "", "error: the sky is the wrong colour\n");
    assert_eq!(classify(&s), BashFailureKind::Unknown);
}

#[test]
fn stdout_content_cannot_exempt_a_failure() {
    // `cat` of a log, or `ls` of a directory, routinely contains these exact
    // phrases as ordinary content. Matching there would exempt a real defect
    // on the strength of a filename.
    let s = snippet(
        1,
        "2026-08-16 connection refused\n2026-08-16 no such file or directory\n",
        "error: the sky is the wrong colour\n",
    );
    assert_eq!(classify(&s), BashFailureKind::Unknown);
}

#[test]
fn a_snippet_with_no_stderr_section_is_still_read() {
    // Plenty of tools write their diagnostic to stdout. With no stderr section
    // to scan, the whole snippet is fair game.
    let s = snippet(1, "fatal: destination path already exists\n", "");
    assert_eq!(classify(&s), BashFailureKind::Unknown);
    let s = snippet(127, "bash: nope: command not found\n", "");
    assert_eq!(classify(&s), BashFailureKind::ModelError);
}

#[test]
fn the_exit_code_is_read_off_the_error_line() {
    assert_eq!(exit_code(&snippet(127, "", "x")), Some(127));
    assert_eq!(exit_code(&snippet(1, "", "x")), Some(1));
    assert_eq!(exit_code("no exit line here"), None);
}

#[test]
fn the_stderr_head_is_the_diagnostic_not_the_stdout() {
    let s = snippet(
        1,
        "lots of unrelated stdout\n",
        "fatal: not a git repository\n",
    );
    assert_eq!(stderr_head(&s, 160), Some("fatal: not a git repository"));
    // No stderr, and whitespace-only stderr, both yield nothing to report.
    assert_eq!(stderr_head(&snippet(1, "out", ""), 160), None);
    assert_eq!(stderr_head(&snippet(1, "", "   \n"), 160), None);
}

#[test]
fn a_long_stderr_head_is_capped_on_a_char_boundary() {
    // Multi-byte input must not panic the slice.
    let s = snippet(1, "", &"é".repeat(300));
    let head = stderr_head(&s, 10).expect("stderr present");
    assert_eq!(head.chars().count(), 10);
}
