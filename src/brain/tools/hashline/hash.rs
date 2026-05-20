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
/// The hash is computed over the line content concatenated with the
/// 1-indexed line number (as ASCII decimal). This ensures blank lines
/// at different positions get different hashes.
///
/// # Arguments
/// * `line_number` - 1-indexed line number
/// * `content` - the line content (without newline)
///
/// # Returns
/// A 2-character string from HASH_ALPHABET.
pub fn hash_line(line_number: usize, content: &str) -> String {
    // Build the hash input: content + "#" + line_number
    let mut input = Vec::with_capacity(content.len() + 8);
    input.extend_from_slice(content.as_bytes());
    input.push(b'#');
    input.extend_from_slice(line_number.to_string().as_bytes());

    let h = fnv1a_32(&input);

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
        .map(|(i, line)| (i + 1, hash_line(i + 1, line)))
        .collect()
}

/// Format a line with its hash tag: `LINE#ID|content`
pub fn format_hashline(line_number: usize, hash: &str, content: &str) -> String {
    format!("{}#{}|{}", line_number, hash, content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_deterministic() {
        let h1 = hash_line(1, "hello world");
        let h2 = hash_line(1, "hello world");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 2);
    }

    #[test]
    fn test_hash_two_chars_from_alphabet() {
        let h = hash_line(42, "some code here");
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
        let h1 = hash_line(1, "hello");
        let h2 = hash_line(1, "world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_different_line_numbers_different_hash() {
        let h1 = hash_line(1, "same content");
        let h2 = hash_line(2, "same content");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_blank_lines_differ_by_position() {
        let h1 = hash_line(1, "");
        let h5 = hash_line(5, "");
        let h100 = hash_line(100, "");
        assert_ne!(h1, h5);
        assert_ne!(h5, h100);
        assert_ne!(h1, h100);
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
        assert_eq!(formatted, "12#VK|function hello() {");
    }

    #[test]
    fn test_empty_content() {
        let h = hash_line(1, "");
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn test_unicode_content() {
        let h = hash_line(1, "héllo wörld 🦀");
        assert_eq!(h.len(), 2);
    }
}
