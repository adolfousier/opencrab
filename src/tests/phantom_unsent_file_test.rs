//! Claiming a file was sent when nothing sent one (#825).
//!
//! Observed: a turn ran `write_file`, four no-ops and `tool_search`, never
//! invoked any send, and opened its reply with "File sent above." The file
//! existed on disk; nothing had carried it to the chat.
//!
//! `all_calls_were_null_effect` does not catch this — `write_file` is genuine
//! work, so the turn kept its exemption despite most calls being theatre. The
//! image equivalent (#747) does not either: it looks for an `<<IMG:>>` marker
//! in the text, because images deliver inline, whereas a document goes out
//! through a tool call. Document sending arrived after those checks were
//! written, which is why the shape was uncovered.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::brain::agent::service::phantom::claims_unsent_file;

fn call(json: &str) -> String {
    json.to_string()
}

/// The turn as it actually ran: real work, no send.
fn wrote_but_never_sent() -> Vec<String> {
    vec![
        call(r#"{"path":"/home/u/report.md","content":"..."}"#),
        call(r#"{"query":"send telegram document file attachment"}"#),
        call(r#"{"command":"true"}"#),
        call(r#"{"command":"ls -la /home/u/report.md"}"#),
    ]
}

#[test]
fn the_observed_claim_is_caught() {
    assert!(claims_unsent_file(
        "File sent above. That delay was me fumbling with placeholder commands.",
        &wrote_but_never_sent()
    ));
}

#[test]
fn a_real_document_send_is_never_flagged() {
    // The honest case must survive, or every genuine delivery gets re-prompted.
    let sent = vec![call(
        r#"{"action":"send_document","document_url":"/home/u/report.md"}"#,
    )];
    assert!(!claims_unsent_file("File sent above.", &sent));
}

#[test]
fn a_search_for_how_to_send_is_not_a_send() {
    // The first version of the marker list included the bare word
    // "attachment", so this turn's own tool_search query — "send telegram
    // document file attachment" — counted as a delivery and the detector
    // silently passed the case it was written for.
    assert!(claims_unsent_file(
        "File sent above.",
        &[call(
            r#"{"query":"send telegram document file attachment"}"#
        )]
    ));
}

#[test]
fn any_upload_shaped_tool_counts_as_delivery() {
    // Matched on input fragments so a new channel's send does not silently
    // fall outside the check.
    for input in [
        r#"{"action":"send_file","path":"/x/r.md"}"#,
        r#"{"action":"upload_file","path":"/x/r.md"}"#,
        r#"{"media_url":"/x/r.md"}"#,
    ] {
        assert!(
            !claims_unsent_file("I attached the report.", &[call(input)]),
            "must count as a real send: {input}"
        );
    }
}

#[test]
fn ordinary_prose_about_a_message_is_not_a_file_claim() {
    // "sent above" is said of plain messages constantly. Without the file
    // context requirement this would flag normal replies.
    assert!(!claims_unsent_file(
        "I sent above a summary of what changed.",
        &wrote_but_never_sent()
    ));
}

#[test]
fn describing_a_file_without_claiming_delivery_is_clean() {
    assert!(!claims_unsent_file(
        "The report is written to /home/u/report.md if you want to read it.",
        &wrote_but_never_sent()
    ));
}

#[test]
fn an_honest_failure_to_send_survives() {
    // Saying it could NOT be sent must never be flagged; the check exists to
    // protect that admission, not punish it.
    assert!(!claims_unsent_file(
        "I could not send the file — the document tool is not available to me.",
        &wrote_but_never_sent()
    ));
}

// ── Multilingual, per the standing rule ─────────────────────────────────────
// Phrases live in every phantom_lang TOML and are scanned as a union, never
// via detect_language, which misreads accented Latin and would let a
// Portuguese or Spanish claim through untouched.

#[test]
fn a_portuguese_claim_is_caught() {
    assert!(claims_unsent_file(
        "Ficheiro enviado acima, com o relatório completo.",
        &wrote_but_never_sent()
    ));
}

#[test]
fn a_spanish_claim_is_caught() {
    assert!(claims_unsent_file(
        "Archivo enviado arriba, con el informe completo.",
        &wrote_but_never_sent()
    ));
}

#[test]
fn a_french_claim_is_caught() {
    assert!(claims_unsent_file(
        "Fichier envoyé ci-dessus, avec le rapport complet.",
        &wrote_but_never_sent()
    ));
}

#[test]
fn a_russian_claim_is_caught() {
    assert!(claims_unsent_file(
        "Файл отправлен выше, полный отчёт.",
        &wrote_but_never_sent()
    ));
}

#[test]
fn a_real_send_stays_clean_in_another_language() {
    let sent = vec![call(
        r#"{"action":"send_document","document_url":"/home/u/report.md"}"#,
    )];
    assert!(!claims_unsent_file("Ficheiro enviado acima.", &sent));
}
