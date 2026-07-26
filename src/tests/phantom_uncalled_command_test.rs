//! A named command the turn never ran (#789).
//!
//! Every other phantom check matches how a fabrication LOOKS — its verbs, its
//! layout, where in the turn it sits — and a model can always look different.
//! Three such fixes landed in one day and it fabricated twice more within the
//! hour, both times while real tools were executing and giving it cover.
//!
//! This check does not infer. The loop knows exactly what it executed, so a
//! sentence claiming a specific command ran is true or false as a matter of
//! fact. The observed case: "Ran `gh issue list --state open` for real this
//! turn. The actual output is above" — while the only occurrence of `gh issue`
//! in the logs was the model's own text block.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::brain::agent::service::phantom::claims_uncalled_commands;

#[test]
fn a_command_the_turn_never_ran_is_caught() {
    // The reported sentence, with unrelated real calls in the same turn.
    let text = "Ran `gh issue list --state open` for real this turn. \
                The actual output is above, and it corrects me precisely:";
    let executed = vec![
        r#"{"command":"grep -rn foo src/"}"#.to_string(),
        r#"{"path":"src/main.rs"}"#.to_string(),
    ];
    let found = claims_uncalled_commands(text, &executed);
    assert_eq!(found, vec!["gh issue list --state open"]);
}

#[test]
fn a_command_that_really_ran_is_clean() {
    let text = "Ran `gh issue list --state open` and here is what came back:";
    let executed = vec![r#"{"command":"gh issue list --state open --json number"}"#.to_string()];
    assert!(claims_uncalled_commands(text, &executed).is_empty());
}

#[test]
fn extra_flags_on_the_real_call_are_not_a_fabrication() {
    // Prose omits pipelines and redirects all the time; matching the whole
    // span would flag honest recaps.
    let text = "Checked with `wc -l src/tui/mod.rs`, real output above:";
    let executed = vec![r#"{"command":"cd /repo && wc -l src/tui/mod.rs | tail -1"}"#.to_string()];
    assert!(claims_uncalled_commands(text, &executed).is_empty());
}

#[test]
fn a_proposed_command_is_not_a_claim() {
    // The framing requirement is what keeps suggestions safe. Offering to run
    // something must never be flagged.
    for proposal in [
        "I could run `gh issue list --state open` if you want the full set.",
        "You can verify with `shasum -a 256 src/main.rs` yourself.",
        "The next step would be `cargo test --all-features` on a clean tree.",
    ] {
        assert!(
            claims_uncalled_commands(proposal, &[]).is_empty(),
            "a proposal must not be flagged: {proposal}"
        );
    }
}

#[test]
fn backticked_prose_is_not_a_command() {
    // Symbols, paths and filenames in backticks have no argument, so they are
    // never mistaken for a command claim.
    let text = "Ran the check and `mod.rs` is declarations only, see `src/tui/mod.rs`.";
    assert!(claims_uncalled_commands(text, &[]).is_empty());
}

#[test]
fn an_unknown_program_is_left_alone() {
    // The program allowlist keeps this conservative: an unrecognised tool is
    // not assumed to be a shell command.
    let text = "Ran `frobnicate the widgets` and it worked.";
    assert!(claims_uncalled_commands(text, &[]).is_empty());
}

#[test]
fn several_claims_are_all_reported() {
    let text = "Ran `git log --oneline -5` and `cargo test --all-features`, output above.";
    let found = claims_uncalled_commands(text, &[]);
    assert_eq!(found.len(), 2, "got: {found:?}");
}

// ── Multilingual (#789) ─────────────────────────────────────────────────────
// The command is language-neutral; the framing that turns a proposal into a
// claim is not. An English-only marker list let a fabrication in any other
// language through untouched, which is the exact failure cd77d138 fixed for
// the strict detector: accents must not disable detection.

#[test]
fn a_claim_in_portuguese_is_caught() {
    let text = "Executei `gh issue list --state open` e a saída está acima.";
    assert_eq!(
        claims_uncalled_commands(text, &[]),
        vec!["gh issue list --state open"]
    );
}

#[test]
fn a_claim_in_spanish_is_caught() {
    let text = "Ejecuté `git log --oneline -5` y la salida está arriba.";
    assert_eq!(
        claims_uncalled_commands(text, &[]),
        vec!["git log --oneline -5"]
    );
}

#[test]
fn a_claim_in_french_is_caught() {
    let text = "J'ai exécuté `cargo test --all-features`, la sortie ci-dessus le confirme.";
    assert_eq!(
        claims_uncalled_commands(text, &[]),
        vec!["cargo test --all-features"]
    );
}

#[test]
fn a_claim_in_russian_is_caught() {
    let text = "Я запустил `git status --short` и вывод выше.";
    assert_eq!(
        claims_uncalled_commands(text, &[]),
        vec!["git status --short"]
    );
}

#[test]
fn a_real_call_in_another_language_stays_clean() {
    // The honest case must survive translation too.
    let text = "Executei `gh issue list --state open` e a saída está acima.";
    let executed = vec![r#"{"command":"gh issue list --state open --json number"}"#.to_string()];
    assert!(claims_uncalled_commands(text, &executed).is_empty());
}
