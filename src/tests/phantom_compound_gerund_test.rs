//! #1193: a work announcement whose qualifying gerund is not the leftmost one
//! must still match.
//!
//! `work_announcement_re` allows only 60 characters between the gerund and its
//! imminence marker. In a compound announcement ("fetching X, then executing
//! Y:") the alternation binds the FIRST gerund, the bounded run then has to
//! span the whole remaining clause, and the match is abandoned even though the
//! second gerund sits 34 characters from the marker. Raising the bound would
//! widen every announcement's tolerance; instead the regex accepts an optional
//! connective ("then", "and then", "next", and each language's equivalents)
//! so the second clause is matched on its own terms.

use crate::brain::agent::service::phantom::{
    has_phantom_tool_intent_no_tools, matches_work_announcement,
};

/// The reported turn verbatim, minus the fenced block that followed it.
const REPORTED_LEAD: &str = "Queue locked: everything except #933/#1112. That's ten \
     issues — fetching all specs fresh before planning, then executing autonomously \
     until the last close:";

#[test]
fn test_second_gerund_carries_the_announcement() {
    assert!(
        matches_work_announcement(REPORTED_LEAD),
        "#1193: 82 chars from the first gerund to the marker, 34 from the second"
    );
}

#[test]
fn test_reported_turn_is_phantom_end_to_end() {
    let turn =
        format!("{REPORTED_LEAD}\n\n```bash\ncd ~/srv/rs/opencrabs && gh issue view 1176\n```");
    assert!(
        has_phantom_tool_intent_no_tools(&turn),
        "#1193: the reported zero-tool turn still reads as a real answer"
    );
}

#[test]
fn test_compound_announcement_in_every_language() {
    // Each language's own connective, scanned across all langs the way every
    // other phantom tell is. Never via detect_language, which misreads
    // accented Latin.
    for (lang, lead) in [
        (
            "en",
            "Ten issues — fetching the specs, then executing them now.",
        ),
        (
            "es",
            "Diez temas — buscando los datos, luego ejecutando todo ahora.",
        ),
        (
            "pt",
            "Dez itens — buscando os dados, depois executando tudo agora.",
        ),
        (
            "fr",
            "Dix points — je récupère les specs, puis j'exécute maintenant.",
        ),
        (
            "id",
            "Sepuluh isu — mengambil spesifikasi, lalu menjalankan sekarang.",
        ),
        (
            "ru",
            "Десять задач — сейчас проверяю специи, затем запускаю сейчас.",
        ),
    ] {
        assert!(
            matches_work_announcement(lead),
            "#1193: compound announcement missed for {lang}: {lead:?}"
        );
    }
}

#[test]
fn test_connective_alone_does_not_make_an_announcement() {
    // The connective is a prefix, not a trigger: without a gerund AND a
    // trailing imminence marker there is still no announcement.
    for lead in [
        "I looked at two things, then wrote up the result.",
        "The build and running tests are separate concerns.",
        "We compared A and B, and the results were identical.",
        "First the config, then the schema, then the migration.",
    ] {
        assert!(
            !matches_work_announcement(lead),
            "#1193: false positive on a bare connective: {lead:?}"
        );
    }
}

#[test]
fn test_marker_bound_is_unchanged() {
    // The fix must not widen per-gerund tolerance. A single gerund still
    // loses its marker past 60 characters.
    let over = "fetching all of the issue specifications fresh from the API before \
                any planning happens at all:";
    assert!(
        !matches_work_announcement(over),
        "#1193: the 60-char bound was widened, not worked around"
    );
}
