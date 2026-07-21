//! Tests for table delivery routing (#651, #679).
//!
//! #679: a table message skips only the doomed native rich-BLOCKS attempt
//! (Telegram's InputRichBlock 400s our table shape) and is sent via rich
//! MARKDOWN, which renders tables correctly. The monospace-HTML converter is
//! the final fallback if the rich send fails; these tests cover both the
//! `contains_table` routing signal and that the HTML fallback renders cleanly.

use crate::channels::telegram::handler::markdown_to_telegram_html;
use crate::channels::telegram::rich::contains_table;

const TABLE_AND_FENCE: &str = "\
| Policy | Behavior |
|---|---|
| **ask** | prompt only for dangerous tools |

```rust
needs_approval = a && b;
```";

#[test]
fn table_is_detected_so_the_blocks_attempt_is_skipped() {
    // delivery.rs skips the native rich-BLOCKS send when `contains_table` is
    // true (blocks 400 on tables) and routes to the rich-markdown send instead,
    // which renders tables. Plain prose is not detected as a table.
    assert!(contains_table(TABLE_AND_FENCE));
    assert!(!contains_table("no table here\njust prose"));
}

#[test]
fn table_plus_fence_renders_clean_html_no_mangling() {
    let html = markdown_to_telegram_html(TABLE_AND_FENCE);
    // The empty-bold artifact from the naive converter must never appear.
    assert!(
        !html.contains("<b></b>"),
        "empty-bold artifact leaked: {html}"
    );
    // The fence-language leak must never appear.
    assert!(
        !html.contains("</code>rust"),
        "fence language leaked: {html}"
    );
    // Monospace rendering is used (table grid and/or fenced code in <pre>).
    assert!(
        html.contains("<pre>"),
        "expected monospace <pre> block: {html}"
    );
    // The && inside the fence is escaped exactly once.
    assert!(html.contains("&amp;&amp;") && !html.contains("&amp;amp;"));
}
