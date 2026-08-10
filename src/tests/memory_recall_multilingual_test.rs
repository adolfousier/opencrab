//! Multilingual scoring for MEMORY.md recall (#996 follow-up).
//!
//! The ranker tokenizes with `char::is_alphanumeric`, which is Unicode-aware,
//! and stems with English-shaped suffixes. Neither had been measured on
//! anything but English, while real traffic in this project spans the six
//! languages the prompt analyzer supports (en, ru, es, pt, fr, id).
//!
//! Two distinct risks, and they need separating:
//!
//! 1. **Tokenization.** Accented Latin and Cyrillic must survive as words. If
//!    they did not, whole languages would score zero and recall would be
//!    silently English-only.
//! 2. **Cross-language bleed.** The corpus says the same things in six
//!    languages, so a query must be answered by ITS language's section. Getting
//!    the topic right in the wrong language is a wrong answer: the rule the
//!    user reads back would be one they cannot act on.

use crate::brain::memory_recall::{RECALL_MAX_CHARS, RECALL_MAX_SECTIONS, RECALL_MIN_SCORE};
use crate::brain::section_rank::Ranked;
use crate::eval::recall::{RecallDataset, RecallReport};

const CORPUS: &str = include_str!("../eval/fixtures/memory_corpus_multilingual.md");
const DATASET: &str = include_str!("../eval/fixtures/memory_recall_multilingual.json");

/// Language tag carried in each section heading and each expected id.
const LANGS: [&str; 6] = ["(en)", "(es)", "(pt)", "(fr)", "(ru)", "(id)"];

fn dataset() -> RecallDataset {
    RecallDataset::from_json(DATASET).expect("fixture parses")
}

fn ids(m: &crate::brain::brain_sections::Matches) -> Vec<String> {
    m.sections
        .iter()
        .map(|s| s.heading.trim_start_matches('#').trim().to_string())
        .collect()
}

fn ranked_results() -> (RecallDataset, Vec<Vec<String>>) {
    let ds = dataset();
    let ranked = Ranked::build(CORPUS);
    let results = ds
        .cases
        .iter()
        .map(|c| {
            ids(&ranked.find_relevant(
                &c.query,
                RECALL_MAX_SECTIONS,
                RECALL_MAX_CHARS,
                RECALL_MIN_SCORE,
            ))
        })
        .collect();
    (ds, results)
}

/// Accented Latin and Cyrillic survive tokenization.
///
/// `is_alphanumeric` is Unicode-aware, so this should hold, but it is the
/// assumption every other result here rests on and it costs one test.
#[test]
fn non_ascii_scripts_are_matchable_at_all() {
    let ranked = Ranked::build(CORPUS);
    for (query, expect) in [
        ("какой статический анализатор запускать", "(ru)"),
        ("quel analyseur statique lancer", "(fr)"),
        ("penganalisis statis dijalankan", "(id)"),
    ] {
        let got = ids(&ranked.find_relevant(query, RECALL_MAX_SECTIONS, RECALL_MAX_CHARS, 0.0));
        assert!(
            !got.is_empty(),
            "'{query}' matched nothing at all, so that script is not tokenizing"
        );
        assert!(
            got[0].contains(expect),
            "'{query}' should rank a {expect} section first, got {got:?}"
        );
    }
}

/// Every language retrieves its own section, not a translation of it.
#[test]
fn a_query_is_answered_in_its_own_language() {
    let (ds, results) = ranked_results();
    let mut wrong_language = Vec::new();

    for (case, got) in ds.cases.iter().zip(results.iter()) {
        if case.is_negative() || got.is_empty() {
            continue;
        }
        let want_lang = LANGS
            .iter()
            .find(|l| case.relevant[0].contains(**l))
            .expect("every positive id carries a language tag");
        if !got[0].contains(want_lang) {
            wrong_language.push(format!("{} -> {}", case.query, got[0]));
        }
    }

    assert!(
        wrong_language.is_empty(),
        "{} query/queries answered in the wrong language:\n  {}",
        wrong_language.len(),
        wrong_language.join("\n  ")
    );
}

/// Recall and silence hold across all six languages, not just English.
#[test]
fn multilingual_recall_matches_the_english_bar() {
    let (ds, results) = ranked_results();
    let report = RecallReport::compute(&ds.cases, &results, RECALL_MAX_SECTIONS);

    assert!(
        report.aggregate.recall >= 0.75,
        "multilingual recall {:.3} is below the bar held for English\n{}",
        report.aggregate.recall,
        report.render()
    );
    assert!(
        report.false_positive_rate() <= 0.25,
        "multilingual over-firing {:.3}\n{}",
        report.false_positive_rate(),
        report.render()
    );
}

/// No single language carries the average while another is broken.
///
/// An aggregate hides a language that scores zero when five others score one.
#[test]
fn no_language_is_left_behind() {
    let (ds, results) = ranked_results();

    for lang in LANGS {
        let mut total = 0usize;
        let mut hit = 0usize;
        for (case, got) in ds.cases.iter().zip(results.iter()) {
            if case.is_negative() || !case.relevant[0].contains(lang) {
                continue;
            }
            total += 1;
            if got.iter().any(|g| g == &case.relevant[0]) {
                hit += 1;
            }
        }
        assert!(total > 0, "no positive cases for {lang}");
        let rate = hit as f64 / total as f64;
        assert!(
            rate >= 0.6,
            "{lang} recall is {rate:.2} ({hit}/{total}), the ranker is not \
             working for that language"
        );
    }
}

// --- diacritics -------------------------------------------------------------

/// An unaccented query matches accented text.
///
/// This was a live defect found by this suite, not a hypothetical. Matching ran
/// on raw tokens, so `operacion` did not match `operación`, `configuracao` did
/// not match `configuração`, and `revision` did not match `révision`. People
/// type without accents constantly, so for Spanish, Portuguese and French the
/// discriminating word in a question routinely matched nothing.
///
/// The corpus below is built so the accented word is the ONLY thing separating
/// the sections: every other word is shared, and therefore carries no signal.
/// An earlier version of this check used ordinary prose and passed before the
/// fix, because the surrounding words were quietly doing the matching.
#[test]
fn an_unaccented_query_still_matches_accented_text() {
    let corpus = "\
# M

## Alpha

La operación de datos en el sistema para el equipo de trabajo aqui descrito.

## Beta

La instalación de datos en el sistema para el equipo de trabajo aqui descrito.

## Gamma

La configuração de datos en el sistema para el equipo de trabajo aqui descrito.

## Delta

La révision de datos en el sistema para el equipo de trabajo aqui descrito.
";
    let ranked = Ranked::build(corpus);
    for (query, expected) in [
        ("operacion", "Alpha"),
        ("instalacion", "Beta"),
        ("configuracao", "Gamma"),
        ("revision", "Delta"),
    ] {
        let got = ids(&ranked.find_relevant(
            query,
            RECALL_MAX_SECTIONS,
            RECALL_MAX_CHARS,
            RECALL_MIN_SCORE,
        ));
        assert_eq!(
            got,
            vec![expected.to_string()],
            "unaccented '{query}' must reach the accented section"
        );
    }
}

/// Folding must not merge distinct Cyrillic letters.
///
/// Under NFD the Russian `й` decomposes to `и` plus a combining breve, so a
/// blanket "drop all combining marks" fold would make `бой` and `бои` the same
/// word. The fold is therefore restricted to marks whose base is ASCII, and
/// this pins that restriction: these are different words and must not collide.
#[test]
fn folding_leaves_cyrillic_distinctions_intact() {
    let corpus = "\
# M

## Первый

Здесь описан бой в системе для команды.

## Второй

Здесь описаны бои в системе для команды.
";
    let ranked = Ranked::build(corpus);
    let got = ids(&ranked.find_relevant(
        "бой",
        RECALL_MAX_SECTIONS,
        RECALL_MAX_CHARS,
        RECALL_MIN_SCORE,
    ));
    assert!(!got.is_empty(), "the Cyrillic query matched nothing at all");
    assert_eq!(
        got[0], "Первый",
        "'бой' must not be folded into 'бои', they are different words"
    );
}
