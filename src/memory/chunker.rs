//! Split a document into overlapping chunks without ever cutting a character
//! (#1002).
//!
//! Replaces `qmd::chunk_document`, which slices by byte index with no boundary
//! check and panics on any multi-byte character landing near a chunk edge:
//!
//! ```text
//! start byte index 2240 is not a char boundary; it is inside '—'
//! ```
//!
//! It applies `max_chars` to `content.len()`, which is bytes, then uses the
//! result as a string index directly. Its break-point search does the same
//! with `slice.len() * 7 / 10`. An em dash, an accented letter, Cyrillic or an
//! emoji anywhere near those offsets is enough, which describes most real
//! content. The panic surfaced inside a tokio worker during backfill and took
//! the process down with it.
//!
//! Same behaviour otherwise: prefer a paragraph break, then a sentence, then a
//! line, then a word; keep an overlap so a passage spanning a boundary survives
//! whole in one chunk; always make forward progress.
//!
//! Positions are counted in CHARACTERS, which is what a caller passing a
//! "chunk size" means. `pos` on the returned chunk stays a BYTE offset, so it
//! keeps matching what is stored in `content_vectors`.

/// One chunk of a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub text: String,
    /// Byte offset of this chunk in the original document.
    pub pos: usize,
}

/// Byte offsets of every character boundary, plus the end of the string.
///
/// Indexing this by character position gives a byte offset that is always safe
/// to slice at, which is the entire defect being fixed.
fn boundaries(content: &str) -> Vec<usize> {
    content
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(content.len()))
        .collect()
}

/// Character index of a byte offset, for mapping a pattern match back.
fn char_index_of(bounds: &[usize], byte: usize) -> usize {
    // bounds is sorted, so the count of boundaries at or below `byte` minus one
    // is its character index.
    bounds.partition_point(|&b| b < byte)
}

/// Split `content` into chunks of at most `max_chars` characters, overlapping
/// by `overlap_chars`.
///
/// Content that already fits returns a single chunk, so short documents are
/// untouched.
pub fn chunk_document(content: &str, max_chars: usize, overlap_chars: usize) -> Vec<Chunk> {
    if content.is_empty() || max_chars == 0 {
        return Vec::new();
    }

    let bounds = boundaries(content);
    let total_chars = bounds.len() - 1;

    if total_chars <= max_chars {
        return vec![Chunk {
            text: content.to_string(),
            pos: 0,
        }];
    }

    // Overlap must leave room to advance, or the loop could stall on a
    // document whose breaks all land in the overlap window.
    let overlap = overlap_chars.min(max_chars.saturating_sub(1));

    let mut chunks = Vec::new();
    let mut start = 0usize; // character index

    while start < total_chars {
        let hard_end = (start + max_chars).min(total_chars);
        let end = if hard_end < total_chars {
            find_break(content, &bounds, start, hard_end).unwrap_or(hard_end)
        } else {
            hard_end
        };
        // A break earlier than the start would go backwards; fall back to the
        // hard cut.
        let end = if end <= start { hard_end } else { end };

        chunks.push(Chunk {
            text: content[bounds[start]..bounds[end]].to_string(),
            pos: bounds[start],
        });

        if end >= total_chars {
            break;
        }

        let next = end.saturating_sub(overlap);
        // Never step backwards or stand still, whatever the overlap asks for.
        start = if next > start { next } else { end };
    }

    chunks
}

/// Best break point between `start` and `end`, as a character index.
///
/// Searches only the last 30% of the window, so a chunk stays close to the
/// requested size rather than ending at the first paragraph break it sees.
/// Returns `None` when the window holds no natural break.
fn find_break(content: &str, bounds: &[usize], start: usize, end: usize) -> Option<usize> {
    let window_chars = end - start;
    if window_chars == 0 {
        return None;
    }
    let search_start_char = start + (window_chars * 7 / 10);
    let search = &content[bounds[search_start_char]..bounds[end]];
    let base = bounds[search_start_char];

    // Paragraph, then sentence, then line, then word. All patterns are ASCII,
    // so a match offset inside `search` is itself a valid boundary.
    if let Some(pos) = search.rfind("\n\n") {
        return Some(char_index_of(bounds, base + pos + 2));
    }
    for pattern in [". ", ".\n", "? ", "?\n", "! ", "!\n"] {
        if let Some(pos) = search.rfind(pattern) {
            return Some(char_index_of(bounds, base + pos + 2));
        }
    }
    if let Some(pos) = search.rfind('\n') {
        return Some(char_index_of(bounds, base + pos + 1));
    }
    if let Some(pos) = search.rfind(' ') {
        return Some(char_index_of(bounds, base + pos + 1));
    }
    None
}
