//! A bare tool name counts when the model claims to have used it (#1262).
//!
//! Asked to use the plan feature and execute, a turn answered "Setting up the
//! plan, then mapping every status consumer before touching anything.",
//! called nothing, and closed. It stayed dead for over two hours until a
//! human nudged it.
//!
//! `mentions_registered_tool` is the check written for precisely that claim:
//! a zero-tool turn that NAMES a registered tool is narrating usage that
//! never happened. It could not fire, because `plan` is a single word and
//! bare names were skipped outright as ordinary prose. The exclusion was
//! sound about the risk and wrong about the remedy: what disambiguates a
//! bare name is not its length but whether the text makes it the object of
//! an action the model attributes to itself.
//!
//! These tests pin both halves: the claim is caught, and an instruction
//! addressed to the user is still left alone.

use crate::brain::agent::service::phantom::mentions_registered_tool;

fn tools() -> Vec<String> {
    [
        "plan",
        "bash",
        "grep",
        "ls",
        "load_brain_file",
        "read_file",
        "telegram_send",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
}

/// The reported shape, with the project's nouns replaced by neutral ones.
#[test]
fn the_reported_claim_is_caught() {
    assert!(
        mentions_registered_tool(
            "Setting up the plan, then mapping every call site before touching anything.",
            &tools()
        ),
        "a turn that announces setting up the plan and calls nothing is claiming a tool it \
         never used"
    );
}

/// The frame is what carries the claim, so it must hold across the forms the
/// same claim takes.
#[test]
fn a_bare_name_as_the_object_of_the_models_own_action_is_caught() {
    for text in [
        "Running bash to collect the versions.",
        "Using the plan to track each step.",
        "I checked the plan and every task is still open.",
        "Reading the config via grep first.",
        "Invoking plan now.",
    ] {
        assert!(
            mentions_registered_tool(text, &tools()),
            "claim not caught: {text}"
        );
    }
}

/// Written as a name rather than as a word.
#[test]
fn a_backticked_or_slashed_name_is_caught() {
    assert!(mentions_registered_tool(
        "The `plan` came back empty.",
        &tools()
    ));
    assert!(mentions_registered_tool(
        "Started it with /plan and moved on.",
        &tools()
    ));
    assert!(mentions_registered_tool(
        "The plan tool reports three open tasks.",
        &tools()
    ));
}

/// The exclusion existed for a real reason, and the reason still stands:
/// these are ordinary sentences, not claims of tool usage.
#[test]
fn ordinary_prose_containing_a_bare_name_is_still_ignored() {
    for text in [
        // Instruction addressed to the user: `it in` sits between the marker
        // and the name, so the name is not what the marker acts on. This is
        // the assertion the original skip was protecting.
        "run it in bash and plan accordingly",
        "The plan is to migrate the schema first.",
        "According to the plan we agreed on, this ships next week.",
        "That approach has a long tail of edge cases.",
        "Nothing in the plan changed.",
    ] {
        assert!(
            !mentions_registered_tool(text, &tools()),
            "false positive on ordinary prose: {text}"
        );
    }
}

/// Read across `all_langs()`, never from a detected language: a model
/// narrating in one language inside a session in another is the same phantom.
#[test]
fn the_claim_is_caught_in_every_supported_language() {
    for (lang, text) in [
        ("en", "Setting up the plan before anything else."),
        ("pt", "Configurando o plan e depois seguindo em frente."),
        ("es", "Configurando el plan antes de tocar nada."),
        ("fr", "Utilisant le plan pour suivre chaque étape."),
        ("ru", "Запускаю bash, чтобы собрать версии."),
        ("id", "Menyiapkan plan sebelum menyentuh apa pun."),
    ] {
        assert!(
            mentions_registered_tool(text, &tools()),
            "{lang}: bare-name claim not caught"
        );
    }
}

/// Multi-word names are unmistakable and keep matching wherever they appear,
/// with no frame required and no substring hits.
#[test]
fn multi_word_names_are_unchanged() {
    assert!(mentions_registered_tool(
        "Both loaded properly into context via load_brain_file.",
        &tools()
    ));
    assert!(!mentions_registered_tool(
        "the loader_read_filesystem module handles this",
        &tools()
    ));
}
