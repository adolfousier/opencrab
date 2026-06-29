use crate::brain::tools::hashline::hash::*;

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
