//! Repeated code blocks are not a loop (#788).
//!
//! A correct answer quoting two near-identical SQL queries — differing only in
//! a table name — tripped streaming repetition detection at 3322 bytes. The
//! stream was terminated mid-word, the truncation recovery published the
//! partial as its own message, and the reply reached the user torn in half.
//!
//! Repeated literal blocks are normal in technical answers: SQL variants,
//! config pairs, before/after diffs, migration pairs. Prose repetition is the
//! real loop signal and must stay detected.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::brain::agent::service::helpers::{detect_text_repetition, strip_fenced_code};

/// Two queries identical apart from the table, the shape that false-fired.
fn twin_sql_answer() -> String {
    let q = |table: &str| {
        format!(
            "```sql\n\
             SELECT\n  \
             COUNT(*) FILTER (WHERE intake->>'roomsMin' IS NULL) AS omitted,\n  \
             COUNT(*) FILTER (WHERE intake->>'roomsMin' = '0') AS zero,\n  \
             COUNT(*) AS total\n\
             FROM {table} WHERE intake IS NOT NULL;\n\
             ```\n"
        )
    };
    format!(
        "First table:\n{}\nNow the second:\n{}\nThat is the whole picture.\n",
        q("leads_table"),
        q("demands_table")
    )
}

#[test]
fn two_near_identical_queries_are_not_a_loop() {
    let answer = twin_sql_answer();
    // The premise: raw, this DOES look like repetition — that is the bug.
    assert!(
        detect_text_repetition(&answer, 120),
        "premise changed: the raw text no longer trips the detector"
    );
    // Masked, the prose around them carries no repetition.
    assert!(
        !detect_text_repetition(&strip_fenced_code(&answer), 120),
        "repeated code blocks must not read as a provider loop"
    );
}

#[test]
fn prose_repetition_is_still_detected() {
    // The real loop signal must survive the mask.
    let looping = "I should check the config now. I should check the config now. \
                   I should check the config now. I should check the config now."
        .repeat(3);
    assert!(detect_text_repetition(&strip_fenced_code(&looping), 60));
}

#[test]
fn a_loop_around_a_code_block_is_still_detected() {
    // Masking the fence must not give a looping model cover by wrapping its
    // repetition in code.
    let text = "Let me try that again exactly as before now.\n\
                ```sh\necho one\n```\n\
                Let me try that again exactly as before now.\n\
                ```sh\necho two\n```\n\
                Let me try that again exactly as before now.\n";
    assert!(detect_text_repetition(&strip_fenced_code(text), 40));
}

#[test]
fn an_unclosed_fence_masks_to_the_end() {
    // Mid-stream the closing fence has usually not arrived yet, so an open
    // fence must mask everything after it rather than nothing.
    let text = "Here is the query:\n```sql\nSELECT 1;\nSELECT 1;\nSELECT 1;\n";
    let masked = strip_fenced_code(text);
    assert!(masked.contains("Here is the query:"));
    assert!(!masked.contains("SELECT 1;"), "got: {masked:?}");
}

#[test]
fn text_without_fences_is_unchanged() {
    let plain = "A plain answer with no code at all.\nSecond line.\n";
    assert_eq!(strip_fenced_code(plain), plain);
}
