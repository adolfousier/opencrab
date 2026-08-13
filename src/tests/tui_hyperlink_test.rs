//! URLs and paths in the TUI must be clickable, including when wrapped (#1031).
//!
//! Before this, nothing emitted OSC 8, so everything clickable was the terminal
//! guessing from rendered text. That fallback only recognises URLs — never file
//! paths — and cannot span a line break, so a wrapped URL was two fragments and
//! neither was a valid URL. Zooming out until it fit one line was the only way
//! to click it.
//!
//! The escape is folded into a cell that already holds one visible grapheme,
//! because the crossterm backend writes symbols verbatim. The terminal renders
//! the escape as zero-width and the grapheme as one column, so the buffer's
//! column accounting is unchanged — the invariant `patching_does_not_change_the_area`
//! exists to keep that true.

use crate::tui::hyperlink::{find_links, linkify};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

fn buffer_with(line: &str, width: u16) -> (Buffer, Rect) {
    let area = Rect::new(0, 0, width, 1);
    let mut buf = Buffer::empty(area);
    for (i, ch) in line.chars().take(width as usize).enumerate() {
        buf[(i as u16, 0)].set_symbol(&ch.to_string());
    }
    (buf, area)
}

fn row_text(buf: &Buffer, area: Rect, y: u16) -> String {
    (0..area.width)
        .map(|x| buf[(area.x + x, y)].symbol().to_string())
        .collect()
}

/// The symptom that started this: a path is never linkified by a terminal.
#[test]
fn an_absolute_path_becomes_a_file_uri() {
    let links = find_links("see /Users/me/src/main.rs for detail", 80);
    assert_eq!(links.len(), 1, "expected exactly one link: {links:?}");
    assert_eq!(links[0].uri, "file:///Users/me/src/main.rs");
}

/// URLs still work, and keep their scheme rather than gaining `file://`.
#[test]
fn a_url_keeps_its_own_scheme() {
    let links = find_links("open https://example.com/x now", 80);
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].uri, "https://example.com/x");
}

/// Prose punctuation after a URL is not part of it.
#[test]
fn trailing_punctuation_is_not_part_of_the_link() {
    for (text, want) in [
        ("see https://example.com.", "https://example.com"),
        ("(https://example.com)", "https://example.com"),
        ("https://example.com,", "https://example.com"),
    ] {
        let links = find_links(text, 80);
        assert_eq!(links.len(), 1, "{text}");
        assert_eq!(links[0].uri, want, "{text}");
    }
}

/// A false positive turns prose into a dead link, so detection stays strict.
#[test]
fn ordinary_prose_is_not_linkified() {
    for text in [
        "use and/or as needed",
        "the src/main.rs file", // relative — not rooted
        "ratio 3/4 of the total",
        "nothing here at all",
    ] {
        assert!(
            find_links(text, 80).is_empty(),
            "should not linkify: {text}"
        );
    }
}

/// A run touching the final column may have been cut by our own wrapping.
#[test]
fn a_run_at_the_last_column_is_marked_as_continuing() {
    let width = 20;
    let links = find_links("https://example.com/a", width);
    assert_eq!(links.len(), 1);
    assert!(
        links[0].continues,
        "a link reaching the last column must be flagged, since the wrap may \
         have split it"
    );
}

/// THE invariant: patching must not change how many columns the row occupies.
///
/// The escape is zero-width to the terminal but lives inside a cell that still
/// holds exactly one grapheme, so the buffer's geometry is untouched. If this
/// ever fails, every layout downstream shifts.
#[test]
fn patching_does_not_change_the_area() {
    let (mut buf, area) = buffer_with("go to https://example.com now", 40);
    let before = buf.area;
    linkify(&mut buf, area);
    assert_eq!(buf.area, before, "linkify must not resize the buffer");
    assert_eq!((0..area.width).len(), 40, "column count must be unchanged");
}

/// The link opens before its first character and closes after its last.
#[test]
fn the_escape_wraps_exactly_the_link_text() {
    let (mut buf, area) = buffer_with("x https://a.co y", 40);
    linkify(&mut buf, area);
    let text = row_text(&buf, area, 0);
    assert!(
        text.contains("\x1b]8;;https://a.co\x1b\\"),
        "no opener: {text:?}"
    );
    assert!(text.contains("\x1b]8;;\x1b\\"), "no closer: {text:?}");
    // The visible characters survive.
    let visible: String = text.replace('\x1b', "");
    assert!(visible.contains("https://a.co"));
}

/// Running twice must not nest links.
#[test]
fn linkify_is_idempotent() {
    let (mut buf, area) = buffer_with("see https://a.co", 40);
    linkify(&mut buf, area);
    let once = row_text(&buf, area, 0);
    linkify(&mut buf, area);
    let twice = row_text(&buf, area, 0);
    assert_eq!(once, twice, "a second pass must change nothing");
}

/// A row with nothing linkable is left byte-identical.
#[test]
fn a_row_without_links_is_untouched() {
    let (mut buf, area) = buffer_with("just some ordinary prose", 40);
    let before = row_text(&buf, area, 0);
    linkify(&mut buf, area);
    assert_eq!(row_text(&buf, area, 0), before);
}

/// Degenerate areas must not panic.
#[test]
fn an_empty_area_is_safe() {
    let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
    linkify(&mut buf, Rect::new(0, 0, 0, 0));
    linkify(&mut buf, Rect::new(0, 0, 10, 0));
}
