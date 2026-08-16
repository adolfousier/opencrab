//! The program allowlist that bounds the uncalled-command check (#789).
//!
//! `claims_uncalled_commands` only looks at backticked spans whose first word
//! is a known program, so whatever is missing from that list is invisible to
//! the one phantom detector that checks fact rather than wording.
//!
//! It shipped with `psql` but not `ps`, and with `sha256sum` but not `stat` or
//! `date`. Those are precisely the commands that produce the small verifiable
//! facts a turn invents when it wants to sound finished: a build timestamp, a
//! process age, a file size. A run that never called `stat` asserted a binary
//! was "built at 20:24:41", named `stat` in backticks, framed it as executed,
//! and passed every check.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::brain::agent::service::phantom::claims_uncalled_commands;

#[test]
fn a_fabricated_stat_timestamp_is_caught() {
    // The observed shape: a filesystem fact stated with confidence, sourced
    // from a command the turn never issued.
    let text = "I ran `stat -f %m target/release/opencrabs` and it was built at 20:24:41";
    let executed = vec![r#"{"command":"git log --oneline -5"}"#.to_string()];
    assert_eq!(
        claims_uncalled_commands(text, &executed),
        vec!["stat -f %m target/release/opencrabs"]
    );
}

#[test]
fn a_stat_that_really_ran_is_clean() {
    let text = "I ran `stat -f %m target/release/opencrabs` and it was built at 20:24:41";
    let executed =
        vec![r#"{"command":"stat -f %m target/release/opencrabs | head -1"}"#.to_string()];
    assert!(claims_uncalled_commands(text, &executed).is_empty());
}

#[test]
fn ps_is_recognised_now_that_psql_always_was() {
    // The gap in miniature: the database client was on the list, the process
    // table was not, so any claim about what is running went unchecked.
    let text = "I ran `ps -o lstart -p 4711` and the process predates the binary";
    assert_eq!(
        claims_uncalled_commands(text, &[]),
        vec!["ps -o lstart -p 4711"]
    );
}

#[test]
fn the_added_programs_are_all_checkable() {
    // One fixture per family added, so a future trim of the list fails here
    // rather than silently reopening the hole.
    //
    // Every fixture is dot-free on purpose: the caller splits text into
    // sentences on `.` before it looks for backticks, so a command carrying a
    // file extension, a domain or `./...` is torn in half and never reaches
    // this list. That gap predates the allowlist and is not what this change
    // fixes; pinning dotted fixtures here would pin the wrong behaviour.
    for cmd in [
        "date -u",
        "du -sh target",
        "df -h",
        "which cargo",
        "jq -r",
        "node --version",
        "go build",
        "rustc --version",
        "rustup show",
        "brew list",
        "pip show requests",
        "systemctl status opencrabs",
        "journalctl -u opencrabs",
        "ssh iolodev uptime",
        "openssl x509 -noout",
        "md5sum -b keyfile",
        "dig +short opencrabs",
        "lsof -i :8931",
        "uname -a",
        "uptime -p",
        "sort -u paths",
        "uniq -c counts",
    ] {
        let text = format!("I ran `{cmd}` and the output is above");
        assert_eq!(
            claims_uncalled_commands(&text, &[]),
            vec![cmd.to_string()],
            "program not on the allowlist: {cmd}"
        );
    }
}

#[test]
fn prose_nouns_are_still_left_alone() {
    // `env`, `file` and `test` are program names AND ordinary English nouns.
    // Adding them would flag an honest recap that happens to backtick a noun
    // phrase, and a false self-heal costs a whole turn. Pinned so the next
    // widening of the list has to argue with this test first.
    for text in [
        "I ran the suite and the `env var` was already set",
        "I checked and the `file path` resolves under the project dir",
        "I ran it and the `test harness` reported nothing new",
    ] {
        assert!(
            claims_uncalled_commands(text, &[]).is_empty(),
            "prose noun flagged as a command: {text}"
        );
    }
}

#[test]
fn a_proposed_inspection_is_not_a_claim() {
    // Widening the allowlist must not turn suggestions into accusations: the
    // executed framing is still required.
    for proposal in [
        "You can confirm with `stat -f %m target/release/opencrabs` yourself.",
        "The cheapest check would be `ps -o lstart -p 4711` before we guess.",
        "I could run `df -h` if disk pressure is the suspect.",
    ] {
        assert!(
            claims_uncalled_commands(proposal, &[]).is_empty(),
            "a proposal must not be flagged: {proposal}"
        );
    }
}

#[test]
fn a_fabricated_inspection_in_another_language_is_caught() {
    // The allowlist is language-neutral; the framing around it is not, and
    // both halves have to hold for the widened list to be worth anything.
    let text = "Executei `stat -f %m opencrabs` e a saída está acima";
    assert_eq!(
        claims_uncalled_commands(text, &[]),
        vec!["stat -f %m opencrabs"]
    );
}
