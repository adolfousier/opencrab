//! The chunker never splits a character (#1002).
//!
//! `qmd::chunk_document` sliced by byte index with no boundary check, so a
//! multi-byte character landing near a chunk edge panicked:
//!
//! ```text
//! start byte index 2240 is not a char boundary; it is inside '—'
//! ```
//!
//! It surfaced inside a tokio worker during backfill and took the process down.
//! These tests drive the shapes that trigger it: a multi-byte character sitting
//! exactly on a boundary, several scripts, and content with no natural break to
//! fall back on.

use crate::memory::chunker::chunk_document;

/// Chunks must reassemble into the original when overlap is off.
fn assert_covers(content: &str, chunks: &[crate::memory::chunker::Chunk]) {
    for c in chunks {
        assert!(
            content.is_char_boundary(c.pos),
            "chunk pos {} is not a character boundary",
            c.pos
        );
        assert_eq!(
            &content[c.pos..c.pos + c.text.len()],
            c.text,
            "chunk text does not match the document at its recorded offset"
        );
    }
}

/// The reported crash: an em dash straddling the chunk boundary.
#[test]
fn an_em_dash_on_the_boundary_does_not_panic() {
    // No spaces or newlines, so no natural break exists and the cut lands on
    // the hard limit, which is where the panic happened.
    let unit = "abcdefghij—";
    let content = unit.repeat(400);

    for size in 90..140 {
        let chunks = chunk_document(&content, size, size / 8);
        assert!(!chunks.is_empty(), "size {size} produced nothing");
        assert_covers(&content, &chunks);
    }
}

/// Several scripts at once, including ones with no ASCII to break on.
#[test]
fn multibyte_scripts_chunk_without_panicking() {
    for sample in [
        "Здесь описан бой в системе для команды. ",
        "La configuración del entorno de producción. ",
        "Le déploiement nécessite une révision préalable. ",
        "🦀 emoji and 中文 mixed with ASCII. ",
    ] {
        let content = sample.repeat(300);
        for size in [64, 128, 500, 3200] {
            let chunks = chunk_document(&content, size, size / 8);
            assert!(!chunks.is_empty());
            assert_covers(&content, &chunks);
        }
    }
}

/// Chunk length is bounded in CHARACTERS, not bytes.
///
/// qmd applied the limit to `content.len()`, so a Cyrillic document produced
/// chunks roughly half the intended size and an emoji-heavy one a quarter.
#[test]
fn chunk_size_is_counted_in_characters() {
    let content = "Здесь описан бой в системе. ".repeat(200);
    let chunks = chunk_document(&content, 100, 10);
    for c in &chunks {
        assert!(
            c.text.chars().count() <= 100,
            "chunk has {} characters, over the limit",
            c.text.chars().count()
        );
    }
}

/// Consecutive chunks overlap, so a passage on a boundary survives whole.
#[test]
fn chunks_overlap_and_always_advance() {
    let content = "Sentence about retrieval quality. ".repeat(300);
    let chunks = chunk_document(&content, 400, 60);

    assert!(chunks.len() > 1);
    for pair in chunks.windows(2) {
        assert!(
            pair[1].pos > pair[0].pos,
            "chunks must advance: {} then {}",
            pair[0].pos,
            pair[1].pos
        );
        assert!(
            pair[1].pos < pair[0].pos + pair[0].text.len(),
            "consecutive chunks must overlap"
        );
    }
}

/// A pathological overlap must not stall the loop.
#[test]
fn an_overlap_larger_than_the_chunk_still_terminates() {
    let content = "x".repeat(5_000);
    let chunks = chunk_document(&content, 100, 10_000);
    assert!(chunks.len() > 1, "must still make progress");
    let last = chunks.last().unwrap();
    assert_eq!(
        last.pos + last.text.len(),
        content.len(),
        "chunking must reach the end of the document"
    );
}

/// Content that fits is returned whole.
#[test]
fn short_content_is_one_chunk() {
    let chunks = chunk_document("A short note.", 3200, 480);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].pos, 0);
    assert_eq!(chunks[0].text, "A short note.");
}

/// Degenerate inputs return nothing rather than panicking or looping.
#[test]
fn empty_or_zero_sized_input_yields_nothing() {
    assert!(chunk_document("", 3200, 480).is_empty());
    assert!(chunk_document("content", 0, 0).is_empty());
}

/// Breaks prefer a paragraph boundary over a mid-sentence cut.
#[test]
fn a_paragraph_break_is_preferred() {
    let head = "First part. ".repeat(30);
    let tail = "Second part. ".repeat(30);
    let content = format!("{head}\n\n{tail}");

    let chunks = chunk_document(&content, head.len() + 10, 5);
    assert!(
        chunks[0].text.trim_end().ends_with("First part."),
        "should have broken at the paragraph, got tail: {:?}",
        &chunks[0].text[chunks[0].text.len().saturating_sub(40)..]
    );
}

/// Canary: the dependency defect that made this module necessary still exists.
///
/// If this stops panicking, qmd has fixed `chunk_document` and
/// `memory::chunker` could be reconsidered in favour of the upstream one. A
/// failure here is therefore good news, not a regression, and this comment is
/// the note explaining that to whoever hits it.
///
/// Verified against the real content that crashed a running binary: qmd panics
/// at `llm.rs:627` on the same input our chunker handles above.
#[test]
#[should_panic(expected = "char boundary")]
fn upstream_chunker_still_panics_on_multibyte_input() {
    let content = "abcdefghij—".repeat(400);
    let _ = qmd::chunk_document(&content, 100, 12);
}
