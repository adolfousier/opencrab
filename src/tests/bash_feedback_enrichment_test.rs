//! Tests for `enrich_metadata` — the snippet enricher that appends
//! the bash command text to the feedback ledger row so RSI can
//! categorize calls by subsystem (git vs python vs docker etc.)
//! instead of treating every `bash` event as one blob (issue #132).
//!
//! After the fix the `meta` column carries `cmd=<command>`, so SQL
//! like `WHERE dimension = 'bash' AND meta LIKE '%cmd=git%'` works
//! against both success and failure rows.

use crate::brain::agent::service::feedback::enrich_metadata;
use serde_json::json;

#[test]
fn bash_failure_leads_with_the_discriminators_then_the_cmd() {
    // Shape changed in #1068. This used to be `<snippet> | cmd=<cmd>`, which
    // put the field this file exists to add at the very END of a string the
    // recorder then caps at 500 chars. A bash failure snippet is the tool's
    // error line plus up to 8000 chars of captured output, so any failure with
    // more than ~430 chars of output silently lost its `cmd=` before it
    // reached the ledger. Short discriminators lead now, so the cap can only
    // eat the tail.
    let input = json!({ "command": "git rebase main" });
    let result = enrich_metadata("bash", Some("Command exited with code 1"), Some(&input));
    assert_eq!(
        result,
        Some(
            "class=unknown | exit=1 | cmd=git rebase main | Command exited with code 1".to_string()
        )
    );
}

#[test]
fn a_bash_failure_row_carries_its_class_and_stderr() {
    // The whole point of the enrichment (#1068): split `tool_failure|bash`
    // by hand or by the RSI pass without inventing a new event type.
    let input = json!({ "command": "curl http://localhost:8931" });
    let snippet = "Command exited with code 7\n\n-- output captured before error --\n\
                   STDERR:\ncurl: (7) Failed to connect to localhost port 8931: Connection refused\n";
    let result = enrich_metadata("bash", Some(snippet), Some(&input)).unwrap();
    assert!(result.starts_with("class=environmental | exit=7 | cmd=curl http://localhost:8931 | "));
    assert!(result.ends_with(
        "stderr_head=curl: (7) Failed to connect to localhost port 8931: Connection refused"
    ));
}

#[test]
fn a_model_error_row_is_labelled_as_one() {
    let input = json!({ "command": "frobnicate --all" });
    let snippet = "Command exited with code 127\n\n-- output captured before error --\n\
                   STDERR:\nbash: frobnicate: command not found\n";
    let result = enrich_metadata("bash", Some(snippet), Some(&input)).unwrap();
    assert!(result.starts_with("class=model_error | exit=127 |"));
}

#[test]
fn bash_success_appends_cmd_so_rsi_can_aggregate_patterns() {
    // Earlier this enricher only fired on failure (commit 2b4d7c86).
    // The success-pattern detection pass added to the RSI cycle
    // needs the command on SUCCESS rows too, otherwise it can't see
    // "agent ran `gh issue comment` successfully 50 times this
    // week" — which is the signal that motivates a tool/skill
    // extraction proposal.
    let input = json!({ "command": "git status" });
    let result = enrich_metadata("bash", None, Some(&input));
    assert_eq!(result, Some("cmd=git status".to_string()));
}

#[test]
fn bash_with_no_snippet_still_emits_cmd() {
    // The user-denied path before execution doesn't have a
    // meaningful error string; the cmd= still carries through so
    // RSI can categorize the denial by subsystem.
    let input = json!({ "command": "docker build ." });
    let result = enrich_metadata("bash", None, Some(&input));
    assert_eq!(result, Some("cmd=docker build .".to_string()));
}

#[test]
fn non_bash_failure_passes_snippet_through_unchanged() {
    // The enrichment is bash-only for now. A failure on
    // `parse_document` keeps its snippet verbatim.
    let input = json!({ "path": "/tmp/x.pdf" });
    let result = enrich_metadata("parse_document", Some("File not found"), Some(&input));
    assert_eq!(result, Some("File not found".to_string()));
}

#[test]
fn bash_without_command_field_falls_back_to_snippet() {
    // Defensive: a malformed bash input shouldn't break the
    // recorder. We get only the original snippet.
    let input = json!({ "something_else": "..." });
    let result = enrich_metadata("bash", Some("Some error"), Some(&input));
    assert_eq!(
        result,
        Some("class=unknown | Some error".to_string()),
        "no cmd= and no exit line, but the row must still say which population it is in"
    );
}

#[test]
fn bash_with_none_input_falls_back_to_snippet() {
    // The user-denied path before execution doesn't have a
    // meaningful input; the recorder should still produce a
    // ledger entry.
    let result = enrich_metadata("bash", Some("user_denied_approval"), None);
    assert_eq!(
        result,
        Some("class=unknown | user_denied_approval".to_string())
    );
}

#[test]
fn empty_command_string_is_not_appended() {
    // Edge: a literal empty command. Don't emit `cmd=` because the
    // subsystem prefix LIKE queries would still match.
    let input = json!({ "command": "" });
    let result = enrich_metadata("bash", Some("error"), Some(&input));
    assert_eq!(result, Some("class=unknown | error".to_string()));
    assert!(!result.unwrap().contains("cmd="));
}

#[test]
fn very_long_command_is_truncated_to_300_chars() {
    let long_cmd = "git push origin main && ".repeat(200); // ~4800 chars
    let input = json!({ "command": long_cmd });
    let result = enrich_metadata("bash", Some("error"), Some(&input)).unwrap();
    // class= (13) + " | cmd=" (7) + truncated command (300) + " | " (3)
    // + snippet (5) = 328 chars.
    assert!(
        result.len() <= 328,
        "command should be capped at 300 chars; got {} char meta: {}",
        result.len(),
        &result[..result.len().min(120)]
    );
    assert!(result.starts_with("class=unknown | cmd=git push"));
}

#[test]
fn snippet_with_special_chars_is_preserved() {
    // Real bash errors contain newlines, quotes, etc. The enricher
    // should not mangle them — the | cmd= delimiter is appended as
    // a marker, not as a normalizer.
    let input = json!({ "command": "ls /nonexistent" });
    let snippet = "ls: cannot access '/nonexistent': No such file or directory\nexit code: 2";
    let result = enrich_metadata("bash", Some(snippet), Some(&input)).unwrap();
    assert!(result.contains("No such file or directory"));
    assert!(result.contains("cmd=ls /nonexistent"));
    assert!(result.contains('\n'));
}

#[test]
fn realistic_git_failure() {
    let input = json!({ "command": "git rebase --continue" });
    let snippet = "error: could not apply abc1234... fix typo\nhint: Resolve conflicts then run git rebase --continue";
    let result = enrich_metadata("bash", Some(snippet), Some(&input)).unwrap();
    assert!(result.starts_with("class=unknown | cmd=git rebase --continue | "));
    assert!(result.contains("error: could not apply"));
}

#[test]
fn realistic_python_module_not_found() {
    let input = json!({ "command": "python3 -c \"import openpyxl\"" });
    let snippet = "ModuleNotFoundError: No module named 'openpyxl'";
    let result = enrich_metadata("bash", Some(snippet), Some(&input)).unwrap();
    assert!(result.contains("ModuleNotFoundError"));
    assert!(result.contains("cmd=python3 -c"));
}

#[test]
fn realistic_timeout() {
    let input = json!({ "command": "cargo build --release", "timeout_secs": 60 });
    let snippet = "Command timed out after 120 seconds";
    let result = enrich_metadata("bash", Some(snippet), Some(&input)).unwrap();
    assert!(result.contains("timed out"));
    assert!(result.contains("cmd=cargo build --release"));
}

#[test]
fn command_with_unicode_is_preserved() {
    // Real-world bash often has paths with accents — Mac users
    // especially. The truncation should respect char boundaries.
    let input = json!({ "command": "ls /Users/José/Documents" });
    let result = enrich_metadata("bash", Some("not found"), Some(&input)).unwrap();
    assert!(result.contains("José"));
}

#[test]
fn non_bash_tool_input_is_ignored_even_if_it_has_command_field() {
    // Some hypothetical other tool could have its own `command`
    // field. We only enrich bash so unrelated tools' metadata
    // stays clean.
    let input = json!({ "command": "some_arg" });
    let result = enrich_metadata("custom_tool", Some("snip"), Some(&input));
    assert_eq!(result, Some("snip".to_string()));
}
