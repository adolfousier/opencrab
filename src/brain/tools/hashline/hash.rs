//! Hash computation for hashline editing.
//!
//! Each line in a file gets a 2-character content hash tag computed via FNV-1a.
//! The hash alphabet uses 16 visually-distinct letters (no O/0, I/l confusion),
//! giving 256 possible hash values per line.

/// 16 visually-distinct uppercase letters for the hash alphabet.
/// Excludes: O (looks like 0), I (looks like l/1), A (looks like 4 in some fonts),
/// C (looks like G), D (looks like 0 in some fonts), E (looks like F),
/// G (looks like 6), H (looks like N in some fonts), L (looks like 1).
///
/// Kept: Z P M Q V R W S N K T X J B Y U
/// Actually let's use the same alphabet as oh-my-pi for consistency:
/// Z P M Q V R W S N K T X J B Y H
const HASH_ALPHABET: &[u8; 16] = b"ZPMQVRWSNKTXJBYH";

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
/// A 2-character string from HASH_ALPHABET.
pub fn hash_line(content: &str) -> String {
    let h = fnv1a_32(content.as_bytes());

    // Extract two 4-bit nibbles for the two hash characters
    let hi = ((h >> 4) & 0xF) as usize;
    let lo = (h & 0xF) as usize;

    format!("{}{}", HASH_ALPHABET[hi] as char, HASH_ALPHABET[lo] as char)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_deterministic() {
        let h1 = hash_line("hello world");
        let h2 = hash_line("hello world");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 2);
    }

    #[test]
    fn test_hash_two_chars_from_alphabet() {
        let h = hash_line("some code here");
        assert_eq!(h.len(), 2);
        for c in h.chars() {
            assert!(
                HASH_ALPHABET.contains(&(c as u8)),
                "char '{}' not in alphabet",
                c
            );
        }
    }

    #[test]
    fn test_different_content_different_hash() {
        let h1 = hash_line("hello");
        let h2 = hash_line("world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_identical_content_same_hash() {
        // Stateless hashing: identical content produces same hash
        // regardless of position. Ambiguity is handled by lazy context
        // escalation in the edit tool (issue #105).
        let h1 = hash_line("same content");
        let h2 = hash_line("same content");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_blank_lines_same_hash() {
        // Blank lines have identical content (""), so same hash.
        // The edit tool disambiguates via line number in the HashRef.
        let h1 = hash_line("");
        let h5 = hash_line("");
        assert_eq!(h1, h5);
    }

    #[test]
    fn test_hash_all_lines() {
        let content = "line one\nline two\nline three";
        let hashes = hash_all_lines(content);
        assert_eq!(hashes.len(), 3);
        assert_eq!(hashes[0].0, 1);
        assert_eq!(hashes[1].0, 2);
        assert_eq!(hashes[2].0, 3);
        assert_eq!(hashes[0].1.len(), 2);
    }

    #[test]
    fn test_format_hashline() {
        let formatted = format_hashline(12, "VK", "function hello() {");
        assert_eq!(formatted, "VK|function hello() {");
    }

    #[test]
    fn test_empty_content() {
        let h = hash_line("");
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn test_unicode_content() {
        let h = hash_line("héllo wörld 🦀");
        assert_eq!(h.len(), 2);
    }
}
