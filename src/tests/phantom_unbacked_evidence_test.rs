//! Quoted evidence must come from a real tool result (#785).
//!
//! Once any tool ran, the phantom gate dropped the past-tense branch entirely,
//! so one trivial call bought immunity for the rest of the turn. Observed:
//! self-heal forced a tool, the model ran an `echo`, then reported the output
//! of two greps it never made, with invented line numbers and symbol names.
//!
//! Verb lists cannot catch this. After a genuine grep, "Grepped and found 34
//! hits" is an honest recap and the sentence is identical when the grep never
//! ran. What differs is whether the CONTENT came from a tool, so this checks
//! content.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::brain::agent::service::phantom::claims_unbacked_evidence;

/// The shape a model produces when it invents a grep dump.
const FABRICATED_DUMP: &str = "Verified just now, real output:\n\
     === LINE COUNT ===\n\
     1916 src/tui/mod.rs\n\
     149:pub struct Tui {\n\
     161:impl Tui {\n\
     171:    pub async fn run(&mut self) -> Result<AppExit> {";

#[test]
fn a_dump_no_tool_produced_is_flagged() {
    // The turn ran something, just not what it claims to be quoting.
    let real = vec!["25 src/tui/mod.rs\n".to_string()];
    assert!(claims_unbacked_evidence(FABRICATED_DUMP, &real));
}

#[test]
fn a_dump_with_no_tools_at_all_is_flagged() {
    assert!(claims_unbacked_evidence(FABRICATED_DUMP, &[]));
}

#[test]
fn quoting_a_real_result_is_clean() {
    // The honest case the exemption exists to protect: the content IS in the
    // tool output, so the same wording and the same shape must pass.
    let real = vec![
        "=== LINE COUNT ===\n1916 src/tui/mod.rs\n149:pub struct Tui {\n161:impl Tui {\n\
         171:    pub async fn run(&mut self) -> Result<AppExit> {"
            .to_string(),
    ];
    assert!(!claims_unbacked_evidence(FABRICATED_DUMP, &real));
}

#[test]
fn a_partially_quoted_result_is_clean() {
    // Trimming a long output down to the relevant lines is normal. Only a
    // MAJORITY being absent counts as fabrication.
    let text = "Here are the hits:\n149:pub struct Tui {\n161:impl Tui {\n999:invented line";
    let real = vec!["149:pub struct Tui {\n161:impl Tui {\n".to_string()];
    assert!(!claims_unbacked_evidence(text, &real));
}

#[test]
fn prose_is_never_examined() {
    // No evidence lines at all: ordinary answers must be untouched even when
    // the turn ran no tools.
    let text = "The module is declarations only, which is why the file is short. \
                I would move the vim layer into its own submodule.";
    assert!(!claims_unbacked_evidence(text, &[]));
}

#[test]
fn a_single_citation_is_not_a_dump() {
    // Below the minimum, a line reference is a citation, not quoted output.
    let text = "The fold logic lives at chat.rs and the anchor is set there.\n\
                149:pub struct Tui {";
    assert!(!claims_unbacked_evidence(text, &[]));
}

#[test]
fn a_timestamp_is_not_an_evidence_line() {
    // "13:32:10" splits on ':' with digits on the left, so a log-shaped
    // sentence must not be mistaken for a grep dump.
    let text = "It happened at 13:32:10 and again later.\n\
                The retry fired immediately after.\n\
                Nothing else ran.";
    assert!(!claims_unbacked_evidence(text, &[]));
}
