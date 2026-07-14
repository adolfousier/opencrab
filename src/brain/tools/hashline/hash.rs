//! Hash computation for hashline editing.
//!
//! Each line in a file gets a [`HASH_LEN`]-character content hash tag computed
//! via FNV-1a. The hash alphabet uses 16 visually-distinct letters (no O/0,
//! I/l confusion), giving `16^HASH_LEN` possible hash values per line.
//!
//! The tag width matters a lot: with a 2-char tag (256 values) even a 20-line
//! read had a ~54% chance of a FALSE collision (distinct lines that happen to
//! share a tag), and a 60-line read ~99.9% — which made `hashline_edit` refuse
//! most real edits. A 4-char tag (65536 values) drops those to ~0.3% / ~4%.
//! Identical-content lines still share a tag at any width (that is inherent and
//! is deferred to `edit_file` via the collision path), but the false collisions
//! that drove the failure rate are gone. See #573.

/// 16 visually-distinct uppercase letters for the hash alphabet.
/// Excludes: O (looks like 0), I (looks like l/1), A (looks like 4 in some fonts),
/// C (looks like G), D (looks like 0 in some fonts), E (looks like F),
/// G (looks like 6), H (looks like N in some fonts), L (looks like 1).
///
/// Kept: Z P M Q V R W S N K T X J B Y U
/// Actually let's use the same alphabet as oh-my-pi for consistency:
/// Z P M Q V R W S N K T X J B Y H
pub(crate) const HASH_ALPHABET: &[u8; 16] = b"ZPMQVRWSNKTXJBYH";

/// Number of characters in a line-hash tag. Widened from 2 to 4 (#573): a
/// 2-char tag has only 256 values, so distinct lines collided constantly and
/// the edit tool refused them. 4 chars gives 65536 values.
pub(crate) const HASH_LEN: usize = 4;

/// FNV-1a 32-bit offset basis.
const FNV_OFFSET_BASIS: u32 = 2_166_136_261;

/// FNV-1a 32-bit prime.
const FNV_PRIME: u32 = 16_777_619;

/// Compute FNV-1a 32-bit hash over the given bytes.
fn fnv1a_32(data: &[u8]) -> u32 {
    let mut hash = FNV_OFFSET_BASIS;
    for &byte in data {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Compute a 2-character hash for a line.
///
/// The hash is computed over the line content only (stateless).
/// This ensures **reference stability**: inserting or deleting lines
/// at the top of a file does not invalidate hashes for the rest.
///
/// Identical content at different positions produces the same hash.
/// Ambiguity is handled by the edit tool via lazy context escalation
/// (see issue #105).
///
/// # Arguments
/// * `content` - the line content (without newline)
///
/// # Returns
/// A [`HASH_LEN`]-character string from HASH_ALPHABET.
pub fn hash_line(content: &str) -> String {
    let h = fnv1a_32(content.as_bytes());

    // Take one 4-bit nibble per tag character, from the high nibbles down, so
    // the tag uses the most-mixed bits of the FNV hash first.
    let mut tag = String::with_capacity(HASH_LEN);
    for i in 0..HASH_LEN {
        let shift = 4 * (HASH_LEN - 1 - i) as u32;
        let nibble = ((h >> shift) & 0xF) as usize;
        tag.push(HASH_ALPHABET[nibble] as char);
    }
    tag
}

/// Compute hashes for all lines in a file content.
///
/// Returns a Vec of (1-indexed line number, 2-char hash) pairs.
pub fn hash_all_lines(content: &str) -> Vec<(usize, String)> {
    content
        .lines()
        .enumerate()
        .map(|(i, line)| (i + 1, hash_line(line)))
        .collect()
}

/// Format a line with its hash tag: `ID|content`
pub fn format_hashline(_line_number: usize, hash: &str, content: &str) -> String {
    format!("{}|{}", hash, content)
}
