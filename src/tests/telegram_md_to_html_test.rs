//! Tests for the hardened md_to_html converter (#650): double-asterisk bold,
//! fenced code blocks, and the escaping that keeps Telegram from rejecting the
//! whole message.

use crate::channels::telegram::handler::md_to_html;

#[test]
fn double_asterisk_bold_no_longer_empties() {
    // The regression: **ask** used to become <b></b>ask<b></b>.
    assert_eq!(md_to_html("**ask**"), "<b>ask</b>");
    assert_eq!(
        md_to_html("Set by **auto-always** default"),
        "Set by <b>auto-always</b> default"
    );
}

#[test]
fn single_asterisk_bold_still_works() {
    assert_eq!(md_to_html("*bold*"), "<b>bold</b>");
}

#[test]
fn inline_code_and_escaping() {
    assert_eq!(md_to_html("`x`"), "<code>x</code>");
    // Literal angle brackets inside code are escaped, not treated as tags.
    assert_eq!(
        md_to_html("`/rename <new title>`"),
        "<code>/rename &lt;new title&gt;</code>"
    );
}

#[test]
fn fenced_code_block_with_language() {
    // ```rust ... ``` used to mangle into <code></code>rust via inline logic.
    let out = md_to_html("```rust\nlet x = 1;\n```");
    assert_eq!(out, "<pre><code>let x = 1;</code></pre>");
    assert!(!out.contains("</code>rust"));
}

#[test]
fn fenced_code_block_no_language() {
    assert_eq!(
        md_to_html("```\nplain\n```"),
        "<pre><code>plain</code></pre>"
    );
}

#[test]
fn amp_lt_gt_are_escaped_outside_markup() {
    assert_eq!(md_to_html("a & b < c > d"), "a &amp; b &lt; c &gt; d");
    // && inside a code fence stays escaped once, never doubled.
    assert_eq!(
        md_to_html("```\nx && y\n```"),
        "<pre><code>x &amp;&amp; y</code></pre>"
    );
}

#[test]
fn unclosed_bold_still_balances() {
    // Streaming can cut a message mid-span; the output must stay balanced HTML
    // so Telegram does not reject it.
    assert_eq!(md_to_html("**oops"), "<b>oops</b>");
}

#[test]
fn cron_path_line_converter_renders_fenced_blocks_issue_530() {
    // #530 repro: cron deliver_to=telegram with a ```bash fence. The line-based
    // converter (what send_markdown_outbox falls back to) renders the fence as
    // <pre><code> instead of shipping raw backticks.
    let html =
        crate::channels::telegram::handler::markdown_to_telegram_html("```bash\necho hello\n```");
    assert_eq!(
        html,
        "<pre><code class=\"language-bash\">echo hello\n</code></pre>"
    );
}
