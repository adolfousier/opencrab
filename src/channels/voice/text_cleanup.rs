//! Text sanitization shared by every TTS provider.
//!
//! Markdown markers must be stripped before synthesis regardless of which
//! engine speaks the text, so this lives outside the `local-tts` gate: a
//! build without Piper still sends cleaned text to the API providers.

/// Clean text for TTS synthesis: strip markdown formatting markers only,
/// keeping all actual content so the engine reads the full response
/// naturally instead of pronouncing backticks, asterisks and link targets.
pub(crate) fn clean_for_tts(text: &str) -> String {
    let mut s = text.to_string();

    // Strip code fence markers (```lang and ```) but keep the code content
    s = s
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.starts_with("```")
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Remove inline backticks but keep content inside
    s = s.replace('`', "");

    // Remove markdown bold/italic markers (**, *, __)
    s = s.replace("**", "");
    s = s.replace("__", "");
    s = s.replace('*', "");

    // Remove markdown headers (# ## ### etc.) but keep text
    s = s
        .lines()
        .map(|line| line.trim_start_matches('#').trim_start())
        .collect::<Vec<_>>()
        .join("\n");

    // Remove markdown links [text](url) → keep text only
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '[' {
            let mut link_text = String::new();
            let mut found_close = false;
            for c in chars.by_ref() {
                if c == ']' {
                    found_close = true;
                    break;
                }
                link_text.push(c);
            }
            if found_close && chars.peek() == Some(&'(') {
                chars.next(); // skip '('
                for c in chars.by_ref() {
                    if c == ')' {
                        break;
                    }
                }
                result.push_str(&link_text);
            } else {
                result.push_str(&link_text);
            }
        } else {
            result.push(ch);
        }
    }
    s = result;

    // Remove bullet markers (- or •) at start of lines
    s = s
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix("- ") {
                rest.trim()
            } else if let Some(rest) = trimmed.strip_prefix("• ") {
                rest.trim()
            } else {
                trimmed
            }
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(". ");

    // Collapse repeated punctuation (!!! → !, ??? → ?)
    let mut prev_punct = false;
    let mut cleaned = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch == '!' || ch == '?' {
            if !prev_punct {
                cleaned.push(ch);
            }
            prev_punct = true;
        } else {
            prev_punct = false;
            cleaned.push(ch);
        }
    }
    s = cleaned;

    // Collapse ellipsis (... → .)
    while s.contains("...") {
        s = s.replace("...", ".");
    }
    while s.contains("..") {
        s = s.replace("..", ".");
    }

    // Collapse multiple whitespace/newlines into single space
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}
