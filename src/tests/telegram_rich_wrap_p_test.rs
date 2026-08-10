//! Regression tests for the wrap_p (`<p>`-wrapped) Telegram HTML dialect used
//! by the rich plan-card path (#997). The rich `sendRichMessage` renderer
//! treats bare newlines as ordinary whitespace, so every block, including
//! list items, quotes, and headings, must carry its own `<p>` (or another
//! block element) or it collapses into one run-on line.

use crate::channels::telegram::rich::{markdown_to_html, markdown_to_html_p};

#[test]
fn wrap_p_ordered_list_items_get_their_own_paragraphs() {
    let html = markdown_to_html_p("1. one\n2. two\n3. three");
    assert_eq!(html, "<p>1. one</p><p>2. two</p><p>3. three</p>");
    assert!(
        !html.contains('\n'),
        "bare newlines collapse in the rich dialect"
    );
}

#[test]
fn wrap_p_bullet_list_items_get_their_own_paragraphs() {
    let html = markdown_to_html_p("- alpha\n- beta");
    assert_eq!(html, "<p>• alpha</p><p>• beta</p>");
    assert!(!html.contains('\n'));
}

#[test]
fn wrap_p_task_list_items_get_their_own_paragraphs() {
    let html = markdown_to_html_p("- [ ] todo\n- [x] done");
    assert_eq!(html, "<p>☐ todo</p><p>☑ done</p>");
    assert!(!html.contains('\n'));
}

#[test]
fn wrap_p_nested_list_keeps_every_item_on_its_own_line() {
    let html = markdown_to_html_p("- outer\n  - inner");
    assert_eq!(html, "<p>• outer</p><p>  • inner</p>");
    assert!(!html.contains('\n'));
}

#[test]
fn wrap_p_non_list_child_under_an_item_is_block_wrapped() {
    let html = markdown_to_html_p("- item\n\n  child para");
    assert_eq!(html, "<p>• item</p><p>child para</p>");
    assert!(!html.contains('\n'));
}

#[test]
fn wrap_p_blockquote_paragraphs_get_their_own_paragraphs() {
    let html = markdown_to_html_p("> one\n>\n> two");
    assert_eq!(html, "<blockquote><p>one</p><p>two</p></blockquote>");
    assert!(!html.contains('\n'));
}

#[test]
fn wrap_p_consecutive_headings_stay_separate() {
    let html = markdown_to_html_p("### A\n### B");
    assert_eq!(html, "<p><b><i>A</i></b></p><p><b><i>B</i></b></p>");
}

#[test]
fn wrap_p_paragraph_followed_by_list_stays_separate() {
    let html = markdown_to_html_p("Intro line\n\n1. one\n2. two");
    assert_eq!(html, "<p>Intro line</p><p>1. one</p><p>2. two</p>");
}

#[test]
fn classic_mode_lists_keep_bare_newlines() {
    // Regression guard: the classic ParseMode::Html path relies on raw \n
    // as real line breaks and must not gain <p> tags.
    assert_eq!(markdown_to_html("1. one\n2. two"), "1. one\n2. two");
    assert_eq!(markdown_to_html("- a\n- b"), "• a\n• b");
    assert_eq!(markdown_to_html("# Hi"), "<b>Hi</b>");
}
