//! Claiming a media file was delivered, with no tool call (#880).
//!
//! Observed: a turn announced *"Saiu do forno! nicole-labial.mp4 atualizado,
//! 41 segundos, 15.6 MB"* with a changelog of what the video contained. It ran
//! no tools and uploaded nothing. The user replied "Onde está nao vejo?".
//!
//! Two gaps, not one:
//!
//! 1. The idiom was absent. "Saiu do forno" is a delivery claim in Portuguese
//!    the way "hot off the press" is in English.
//! 2. `file_context_words` listed only document types (`.md`, `pdf`, `csv`) in
//!    ALL six languages, so a claimed `.mp4` could never satisfy the context
//!    half of the check even with a matching phrase.
//!
//! Phrases live in every phantom_lang TOML and are scanned across all of them,
//! never via detect_language, which misreads accented Latin.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::brain::agent::service::phantom::claims_unsent_file;

/// A turn that invoked nothing — the condition the detector exists for.
const NO_TOOLS: &[String] = &[];

#[test]
fn the_observed_portuguese_claim_is_caught() {
    assert!(claims_unsent_file(
        "Saiu do forno! nicole-labial.mp4 atualizado, 41 segundos, 15.6 MB.",
        NO_TOOLS,
    ));
}

#[test]
fn the_same_claim_in_each_language_is_caught() {
    for claim in [
        "Fresh out of the oven! report-final.mp4 updated, 41 seconds.",
        "¡Recién salido del horno! video-final.mp4 actualizado.",
        "Tout juste sorti du four ! la vidéo finale.mp4 est prête.",
        "Baru keluar dari oven! video-final.mp4 sudah diperbarui.",
        "С пылу с жару! видео final.mp4 обновлено.",
    ] {
        assert!(claims_unsent_file(claim, NO_TOOLS), "not caught: {claim}");
    }
}

#[test]
fn a_media_extension_now_satisfies_the_context_check() {
    // The structural half: before this, only document types counted, so any
    // audio or video claim slipped through regardless of wording.
    for ext in [".mp4", ".mp3", ".wav", ".png", ".jpg"] {
        let claim = format!("Saiu do forno! o ficheiro track{ext} está pronto.");
        assert!(
            claims_unsent_file(&claim, NO_TOOLS),
            "extension {ext} not covered"
        );
    }
}

#[test]
fn document_claims_still_work() {
    // Guard against widening breaking the original #825 behaviour.
    assert!(claims_unsent_file(
        "I attached the file, see report.pdf above.",
        NO_TOOLS
    ));
    assert!(claims_unsent_file("Enviei o relatório em anexo.", NO_TOOLS));
}

#[test]
fn an_offer_to_produce_a_file_is_not_a_claim() {
    // Proposing must never be flagged; that is the behaviour to encourage.
    for proposal in [
        "Posso gerar o mp4 se quiser.",
        "Want me to render the video and send it?",
        "Assim que sair, mando o ficheiro.",
    ] {
        assert!(
            !claims_unsent_file(proposal, NO_TOOLS),
            "a proposal was flagged: {proposal}"
        );
    }
}

#[test]
fn ordinary_prose_about_files_is_untouched() {
    assert!(!claims_unsent_file(
        "The render pipeline writes an mp4 into the output directory.",
        NO_TOOLS,
    ));
    assert!(!claims_unsent_file(
        "O bolo saiu do forno e ficou muito bom.",
        NO_TOOLS
    ));
}
