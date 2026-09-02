//! The two Telegram markdown parsers must agree on what a heading is (#1257).
//!
//! `markdown_to_telegram_html` used to treat ANY line starting with `#` as a
//! header: it stripped the hash run and bolded the remainder. Every receipt
//! line that opens with an issue reference (`#1257 closed in abc1234`) came
//! out as bold text with the number deleted — the one token the line existed
//! to carry. The rich gate's `is_atx_heading` had always required a space
//! after the hashes, so the same line took two different shapes depending on
//! which path a message happened to route through.

use crate::channels::telegram::markdown::markdown_to_telegram_html;
use crate::channels::telegram::rich::is_atx_heading;

#[test]
fn issue_reference_is_not_a_heading() {
    // Both hashes and number survive; nothing is bolded.
    let out = markdown_to_telegram_html("#1257 closed in abc1234");
    assert_eq!(out.trim_end(), "#1257 closed in abc1234");
    assert!(!out.contains("<b>"), "receipt line was promoted: {out}");
}

#[test]
fn receipt_block_keeps_every_number() {
    let out = markdown_to_telegram_html("#1255 fixed\n#1256 fixed\n#1257 fixed");
    for n in ["#1255", "#1256", "#1257"] {
        assert!(out.contains(n), "{n} was swallowed: {out}");
    }
    assert!(!out.contains("<b>"), "receipts were promoted: {out}");
}

#[test]
fn real_headings_still_render_bold() {
    assert_eq!(
        markdown_to_telegram_html("# Heading").trim_end(),
        "<b>Heading</b>"
    );
    assert_eq!(
        markdown_to_telegram_html("### Deep heading").trim_end(),
        "<b>Deep heading</b>"
    );
    // Indented headings keep working too.
    assert_eq!(
        markdown_to_telegram_html("  ## Indented").trim_end(),
        "<b>Indented</b>"
    );
}

#[test]
fn seven_hashes_and_bare_hash_stay_literal() {
    // CommonMark caps ATX at six hashes; a bare `#` is not a heading either.
    let seven = markdown_to_telegram_html("####### too deep");
    assert_eq!(seven.trim_end(), "####### too deep");
    let bare = markdown_to_telegram_html("#");
    assert_eq!(bare.trim_end(), "#");
}

#[test]
fn hash_lines_inside_fences_are_untouched() {
    let out = markdown_to_telegram_html("```\n# not a heading\n#1257 ref\n```");
    assert!(out.contains("# not a heading"), "fence body changed: {out}");
    assert!(out.contains("#1257 ref"), "fence body changed: {out}");
    assert!(!out.contains("<b>"), "fence body was promoted: {out}");
}

/// The agreement itself: for every line shape, the HTML ladder promotes to a
/// header exactly when the rich gate calls it a heading. This is the property
/// that broke, so it is asserted directly rather than inferred from samples.
#[test]
fn both_parsers_agree_on_every_shape() {
    let cases = [
        "# heading",
        "###### six",
        "####### seven",
        "#",
        "#1257 receipt",
        "#tag",
        "## ",
        "no hash at all",
    ];
    for case in cases {
        let promoted = markdown_to_telegram_html(case).contains("<b>");
        assert_eq!(
            promoted,
            is_atx_heading(case.trim_start()),
            "parsers disagree on {case:?}"
        );
    }
}
