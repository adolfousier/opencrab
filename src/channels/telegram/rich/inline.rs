//! Inline markdown parsing: `code`, `$math$`, **bold**, ~~strike~~,
//! *italic*/_italic_, and `[text](url)` links.
//!
//! Delimiters are matched non-greedily against their nearest close. Anything
//! unbalanced is emitted as literal text, so malformed markdown degrades to
//! plain text rather than being dropped. `code` and `$math$` spans are literal
//! and never re-parsed; the styling spans recurse so nesting works.

use super::ast::Inline;

/// Parse a single line / cell of markdown into inline spans.
pub(super) fn parse_inlines(input: &str) -> Vec<Inline> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut i = 0;

    while i < input.len() {
        let rest = &input[i..];

        // Literal spans first: their contents are never re-parsed.
        if let Some(c) = rest.strip_prefix('`')
            && let Some(close) = c.find('`')
        {
            flush(&mut text, &mut out);
            out.push(Inline::Code(c[..close].to_string()));
            i += 1 + close + 1;
            continue;
        }
        if !rest.starts_with("$$")
            && let Some(c) = rest.strip_prefix('$')
            && let Some(close) = c.find('$')
            && close > 0
        {
            flush(&mut text, &mut out);
            out.push(Inline::Math(c[..close].to_string()));
            i += 1 + close + 1;
            continue;
        }

        // Paired styling spans (contents recurse).
        if let Some(span) = paired(rest, "**") {
            flush(&mut text, &mut out);
            out.push(Inline::Bold(parse_inlines(span)));
            i += span.len() + 4;
            continue;
        }
        // Underscore emphasis only at word boundaries, so snake_case
        // identifiers like `custom_openai_compatible` are never italicized
        // (and their underscores never eaten). CommonMark forbids intra-word
        // `_` emphasis for exactly this reason.
        if let Some(span) = paired_word_bounded(input, i, rest, "__") {
            flush(&mut text, &mut out);
            out.push(Inline::Bold(parse_inlines(span)));
            i += span.len() + 4;
            continue;
        }
        if let Some(span) = paired(rest, "~~") {
            flush(&mut text, &mut out);
            out.push(Inline::Strike(parse_inlines(span)));
            i += span.len() + 4;
            continue;
        }
        if let Some(span) = paired(rest, "*") {
            flush(&mut text, &mut out);
            out.push(Inline::Italic(parse_inlines(span)));
            i += span.len() + 2;
            continue;
        }
        if let Some(span) = paired_word_bounded(input, i, rest, "_") {
            flush(&mut text, &mut out);
            out.push(Inline::Italic(parse_inlines(span)));
            i += span.len() + 2;
            continue;
        }

        // Links: [text](url)
        if rest.starts_with('[')
            && let Some(link) = parse_link(rest)
        {
            let (content, url, consumed) = link;
            flush(&mut text, &mut out);
            out.push(Inline::Link {
                content: parse_inlines(content),
                url: url.to_string(),
            });
            i += consumed;
            continue;
        }

        // Default: consume one char as literal text.
        let ch = rest.chars().next().unwrap();
        text.push(ch);
        i += ch.len_utf8();
    }

    flush(&mut text, &mut out);
    out
}

/// Push the accumulated literal `text` as a [`Inline::Text`] span and clear it.
fn flush(text: &mut String, out: &mut Vec<Inline>) {
    if !text.is_empty() {
        out.push(Inline::Text(std::mem::take(text)));
    }
}

/// Like [`paired`], but only matches when the delimiter sits at a word
/// boundary on both sides — the char immediately before the opening delimiter
/// and the char immediately after the closing delimiter must not be
/// alphanumeric. Keeps intra-word underscores (snake_case) literal.
fn paired_word_bounded<'a>(input: &str, i: usize, rest: &'a str, delim: &str) -> Option<&'a str> {
    if input[..i]
        .chars()
        .next_back()
        .is_some_and(char::is_alphanumeric)
    {
        return None;
    }
    let span = paired(rest, delim)?;
    let after_close = &rest[delim.len() + span.len() + delim.len()..];
    if after_close
        .chars()
        .next()
        .is_some_and(char::is_alphanumeric)
    {
        return None;
    }
    Some(span)
}

/// If `rest` opens with `delim`, return the substring up to (but not
/// including) the matching closing `delim`. Requires a non-empty body so a
/// lone `**` or stray `_word` stays literal.
fn paired<'a>(rest: &'a str, delim: &str) -> Option<&'a str> {
    let after = rest.strip_prefix(delim)?;
    let close = after.find(delim)?;
    if close == 0 {
        return None;
    }
    Some(&after[..close])
}

/// Parse `[text](url)` at the start of `rest`. Returns the link text, the url,
/// and the total bytes consumed.
fn parse_link(rest: &str) -> Option<(&str, &str, usize)> {
    let mid = rest.find("](")?;
    let end = rest[mid + 2..].find(')')?;
    let text = &rest[1..mid];
    let url = &rest[mid + 2..mid + 2 + end];
    if url.is_empty() {
        return None;
    }
    Some((text, url, mid + 2 + end + 1))
}
