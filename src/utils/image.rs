/// Extract `<<IMG:path>>` markers from text.
///
/// Returns `(cleaned_text, vec_of_paths)` — the text has all markers removed
/// and trimmed, the vec contains the file paths in order of appearance.
pub fn extract_img_markers(text: &str) -> (String, Vec<String>) {
    extract_markers_with_prefix(text, "<<IMG:")
}

/// Extract `<<VID:path>>` markers from text — mirror of `extract_img_markers`
/// for video attachments. Used by channel handlers to strip the marker from
/// bot replies before display (the agent shouldn't normally echo it back, but
/// strip defensively so a leaking marker never lands in front of the user).
pub fn extract_vid_markers(text: &str) -> (String, Vec<String>) {
    extract_markers_with_prefix(text, "<<VID:")
}

/// Extract `<<react:emoji>>` directive from text.
///
/// Returns `(cleaned_text, Option<emoji>)` — valid directives are removed
/// (text trimmed) and the first extracted emoji is returned. Multiple valid
/// directives are all stripped but only the first emoji is returned.
///
/// The LLM outputs `<<react:👍>>` to signal a reaction-only response
/// (or a reaction alongside text). Channel handlers use the returned
/// emoji to call `set_message_reaction` on the user's message.
///
/// Unlike the `<<IMG:path>>` extractor this is deliberately strict, because
/// the marker can legitimately appear in PROSE when the agent talks about
/// the feature itself (docs, code review, this codebase). Two guards:
/// * the payload must look like an actual emoji (non-empty, ≤ 8 chars, no
///   ASCII) — `<<react:emoji>>` or `<<react:hello>>` written in prose stays
///   in the text and produces no reaction (a word payload once fired a bogus
///   REACTION_INVALID Telegram call and mutated the final text, breaking
///   exact-match dedup against the already-sent intermediate: both copies
///   of the message landed in the chat);
/// * occurrences inside backtick code spans are never treated as directives.
pub fn extract_react_marker(text: &str) -> (String, Option<String>) {
    const PREFIX: &str = "<<react:";
    let mut out = String::with_capacity(text.len());
    let mut emoji: Option<String> = None;
    let mut in_code = false;
    let mut i = 0;

    while i < text.len() {
        let ch = text[i..].chars().next().expect("i lies on a char boundary");
        if ch == '`' {
            in_code = !in_code;
            out.push(ch);
            i += 1;
            continue;
        }
        if !in_code
            && text[i..].starts_with(PREFIX)
            && let Some(rel_end) = text[i..].find(">>")
        {
            let payload = text[i + PREFIX.len()..i + rel_end].trim();
            if is_reaction_emoji(payload) {
                if emoji.is_none() {
                    emoji = Some(payload.to_string());
                }
                i += rel_end + 2; // past ">>"
                continue;
            }
        }
        out.push(ch);
        i += ch.len_utf8();
    }

    (out.trim().to_string(), emoji)
}

/// A plausible reaction emoji: non-empty, short (compound emoji with skin
/// tones / VS-16 / ZWJ stay under 8 chars), and containing no ASCII — which
/// rejects words and placeholders like "emoji" or "hello" that appear when
/// the marker is mentioned in prose rather than used as a directive.
fn is_reaction_emoji(payload: &str) -> bool {
    !payload.is_empty() && payload.chars().count() <= 8 && payload.chars().all(|c| !c.is_ascii())
}

/// Generic `<<PREFIX:path>>` marker extractor. Walks the text, removes every
/// `<<PREFIX:...>>` occurrence, and collects the inner paths in order. UTF-8
/// safe (works on byte indices that lie on char boundaries — `find`/`replace_range`
/// handle that correctly for the ASCII delimiters used here).
fn extract_markers_with_prefix(text: &str, prefix: &str) -> (String, Vec<String>) {
    let mut out = text.to_string();
    let mut paths = Vec::new();
    let prefix_len = prefix.len();

    while let Some(start) = out.find(prefix) {
        let Some(rel_end) = out[start..].find(">>") else {
            break;
        };
        let end = start + rel_end + 2; // past ">>"
        let path = out[start + prefix_len..start + rel_end].trim().to_string();
        if !path.is_empty() {
            paths.push(path);
        }
        out.replace_range(start..end, "");
    }

    (out.trim().to_string(), paths)
}
