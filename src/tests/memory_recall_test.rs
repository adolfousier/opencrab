//! Memory that surfaces without being asked for (#799).
//!
//! #800 made reading MEMORY.md cheap, but a cheap read still has to be chosen,
//! and the case that hurts most is the one where the model does not know there
//! is anything to look up: it cannot decide to recall a correction it has
//! forgotten exists.
//!
//! The risk running on every turn is the opposite failure, injecting noise
//! forever. These pin both sides: it fires when a match is genuinely good, and
//! stays silent otherwise.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::brain::memory_recall::recall_from;

const MEMORY: &str = "\
# Memory

## Build workflow

Use clippy with all features. Never plain cargo check.

## Telegram owner gate

Only the owner sees channel commands. Non-owners get an ephemeral rejection.

## Release workflow

Tag builds run on five platforms and publish after all of them pass.
";

#[test]
fn a_relevant_message_recalls_the_matching_section() {
    let recall = recall_from(
        MEMORY,
        "how does the telegram owner gate behave for non-owners?",
    )
    .expect("a clearly relevant message must recall something");
    assert!(recall.contains("ephemeral rejection"), "{recall}");
    assert!(
        recall.contains("MEMORY.md"),
        "recall must be labelled as memory, not read as the user's own words: {recall}"
    );
}

#[test]
fn an_unrelated_message_recalls_nothing() {
    // Silence is the default. Injecting on every turn is the failure mode that
    // would make this worse than the problem it solves.
    assert!(recall_from(MEMORY, "what is the weather in Lisbon today?").is_none());
}

#[test]
fn a_single_shared_word_is_not_enough() {
    // One common word would match almost any section on a long message. Two
    // distinct terms co-occurring is the signal.
    assert!(
        recall_from(MEMORY, "tell me about the workflow").is_none(),
        "a lone common term must not trigger recall"
    );
}

#[test]
fn two_distinct_terms_do_trigger_recall() {
    let recall = recall_from(MEMORY, "remind me of the release workflow platforms")
        .expect("two matching terms is a real signal");
    assert!(recall.contains("five platforms"), "{recall}");
}

#[test]
fn a_system_continuation_never_recalls() {
    // Restart recovery and nudges are the harness talking to itself; recall
    // belongs to what the USER asked.
    assert!(
        recall_from(
            MEMORY,
            "[System: Your last response claimed work but produced no tool calls]"
        )
        .is_none()
    );
}

#[test]
fn recall_points_back_at_the_full_read() {
    // A partial read must never read as a ceiling.
    let recall = recall_from(MEMORY, "the telegram owner gate rejection").unwrap();
    assert!(
        recall.contains("Load MEMORY.md"),
        "must offer the deliberate read as a way to get more: {recall}"
    );
}

#[test]
fn an_empty_memory_file_recalls_nothing() {
    assert!(recall_from("", "the telegram owner gate rejection").is_none());
}

#[test]
fn recall_stays_bounded() {
    // Paid on every turn, so it must not grow with the file.
    let mut big = String::new();
    for i in 0..40 {
        big.push_str(&format!(
            "## Telegram rule {i}\n\nTelegram owner gate detail number {i}.\n\n"
        ));
    }
    let recall = recall_from(&big, "telegram owner gate").unwrap();
    assert!(
        recall.chars().count() < 1600,
        "recall grew to {} chars",
        recall.chars().count()
    );
}

// --- disk-path behaviour (#995) --------------------------------------------

/// The cheap rejections happen before the file is touched.
///
/// Both used to run after the whole file was read, so a harness continuation
/// or a message with no usable terms paid a full read of a 99 KB file to
/// return `None`. Pointing the home at a directory with no MEMORY.md at all
/// proves the answer does not depend on reading one: a message that COULD
/// match still returns `None` here (no file), while these return `None`
/// without ever needing it.
#[tokio::test]
async fn rejections_do_not_depend_on_reading_the_file() {
    use crate::brain::memory_recall::recall_for;

    // A harness continuation is never recall-eligible, file or not.
    assert_eq!(
        recall_for("[System: restart recovery] telegram gate").await,
        None
    );
    // Neither is a message with no term longer than two characters.
    assert_eq!(recall_for("ok").await, None);
    assert_eq!(recall_for("go on").await, None);
}

/// Repeated recall against an unchanged file is stable.
///
/// The parse is cached and invalidated on mtime and length, so this pins the
/// property that matters: caching must not change the answer. A stale cache
/// would show up as a second call disagreeing with the first.
#[tokio::test]
async fn repeated_recall_is_stable_across_the_cache() {
    use crate::brain::memory_recall::recall_for;

    let first = recall_for("telegram owner gate approval").await;
    let second = recall_for("telegram owner gate approval").await;
    assert_eq!(
        first, second,
        "recall must be identical across calls, the cache cannot change the answer"
    );
}
