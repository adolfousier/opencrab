//! Scored before/after for MEMORY.md recall (#996).
//!
//! The precision problem was diagnosed against a real workspace, which cannot
//! be committed: real messages and a real MEMORY.md carry identifiers. So the
//! corpus and the queries here are synthetic, written to reproduce the SHAPE of
//! the failure rather than its content. Positives are topical questions with a
//! known answer; negatives are conversational messages whose correct result is
//! silence, which is what real traffic is mostly made of.
//!
//! Negatives are the point. Retrieval that fires on everything scores perfectly
//! on positives alone, which is exactly how the old rule looked defensible while
//! injecting sections into ~9 turns out of 10.

use crate::brain::brain_sections::find_sections_with;
use crate::brain::section_rank::Ranked;
use crate::eval::recall::{RecallDataset, RecallReport};

const CORPUS: &str = include_str!("../eval/fixtures/memory_corpus.md");
const DATASET: &str = include_str!("../eval/fixtures/memory_recall.json");

// The SHIPPED constants, not copies. A local copy drifts the moment the real
// threshold is tuned, and then the eval scores a configuration nobody runs.
use crate::brain::memory_recall::{RECALL_MAX_CHARS, RECALL_MAX_SECTIONS, RECALL_MIN_SCORE};
const MAX_SECTIONS: usize = RECALL_MAX_SECTIONS;
const MAX_CHARS: usize = RECALL_MAX_CHARS;
const MIN_SCORE: f64 = RECALL_MIN_SCORE;
/// The rule BM25 replaced: at least two distinct matching terms.
const OLD_MIN_HITS: usize = 2;

fn dataset() -> RecallDataset {
    RecallDataset::from_json(DATASET).expect("fixture parses")
}

/// A returned section's id is its heading text, matching the fixture labels.
fn ids(m: &crate::brain::brain_sections::Matches) -> Vec<String> {
    m.sections
        .iter()
        .map(|s| s.heading.trim_start_matches('#').trim().to_string())
        .collect()
}

fn report_for(rank: impl Fn(&str) -> Vec<String>) -> RecallReport {
    let ds = dataset();
    let results: Vec<Vec<String>> = ds.cases.iter().map(|c| rank(&c.query)).collect();
    RecallReport::compute(&ds.cases, &results, MAX_SECTIONS)
}

fn old_report() -> RecallReport {
    report_for(|q| {
        ids(&find_sections_with(
            CORPUS,
            q,
            MAX_SECTIONS,
            MAX_CHARS,
            OLD_MIN_HITS,
        ))
    })
}

fn new_report() -> RecallReport {
    let ranked = Ranked::build(CORPUS);
    report_for(|q| ids(&ranked.find_relevant(q, MAX_SECTIONS, MAX_CHARS, MIN_SCORE)))
}

#[test]
fn the_fixture_has_both_positives_and_negatives() {
    let ds = dataset();
    let negatives = ds.cases.iter().filter(|c| c.is_negative()).count();
    let positives = ds.cases.len() - negatives;
    assert!(
        positives >= 10 && negatives >= 10,
        "a dataset without a real negative set cannot see over-firing: \
         {positives} positives, {negatives} negatives"
    );
}

/// The regression this fixes: the old rule answers messages meant to get none.
///
/// The bar is 0.4, not the 0.895 measured on a real workspace. This synthetic
/// fixture UNDER-reproduces the failure: its negatives are short and clean,
/// where real conversational messages are longer and share more incidental
/// words with more sections. So treat the fixture as a conservative floor, and
/// the real improvement as larger than what is scored here.
#[test]
fn the_old_rule_fires_on_messages_that_should_get_silence() {
    let old = old_report();
    assert!(
        old.false_positive_rate() > 0.4,
        "expected the hit-count rule to over-fire on conversational messages, \
         got FPR={:.3}. If this now passes, the fixture stopped reproducing \
         the failure and the comparison below is no longer meaningful.",
        old.false_positive_rate()
    );
}

/// BM25 must cut over-firing sharply.
#[test]
fn bm25_stays_silent_on_conversational_messages() {
    let new = new_report();
    assert!(
        new.false_positive_rate() <= 0.25,
        "BM25 recall fired on {}/{} messages that should have got silence (FPR={:.3})\n{}",
        new.false_positive_rate() * new.negatives as f64,
        new.negatives,
        new.false_positive_rate(),
        new.render()
    );
}

/// ...without buying that silence by going quiet on real questions.
///
/// This is the half that a naive threshold fails: every hit-count variant
/// measured against the real corpus traded noise for genuine recall roughly one
/// for one, which recreates the problem automatic recall exists to solve.
#[test]
fn bm25_still_answers_topical_questions() {
    let new = new_report();
    assert!(
        new.aggregate.recall >= 0.75,
        "BM25 recall on topical questions dropped to {:.3}\n{}",
        new.aggregate.recall,
        new.render()
    );
}

/// The trade, stated honestly: BM25 is quieter and far more precise, and it
/// costs a little recall.
///
/// It does NOT dominate the old rule. On this fixture the old rule finds every
/// answer (recall 1.000) because it answers nearly everything, which is also
/// why its precision is 0.625 and it fires on 5 of 12 messages that wanted
/// silence. BM25 gives up one of twelve answers to roughly halve the noise and
/// lift precision to 0.917.
///
/// The recall floor is a bound on that trade, not a target. If a future change
/// buys silence by going quiet on real questions, this fails.
#[test]
fn bm25_trades_a_little_recall_for_much_better_precision() {
    let old = old_report();
    let new = new_report();

    assert!(
        new.false_positive_rate() < old.false_positive_rate(),
        "BM25 must fire less on silence cases.\nold: {}new: {}",
        old.render(),
        new.render()
    );
    assert!(
        new.aggregate.precision > old.aggregate.precision,
        "BM25 must be more precise on topical questions.\nold: {}new: {}",
        old.render(),
        new.render()
    );
    assert!(
        new.aggregate.recall + 0.1 >= old.aggregate.recall,
        "BM25 gave up more than a tenth of topical recall ({:.3} vs {:.3}), \
         which is no longer a trade worth making.\nold: {}new: {}",
        new.aggregate.recall,
        old.aggregate.recall,
        old.render(),
        new.render()
    );
}
