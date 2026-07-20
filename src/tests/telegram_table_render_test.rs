//! Tests that tables are routed off the broken native rich-blocks path (#651)
//! and render cleanly through the monospace-HTML converter instead.

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
fn table_is_detected_so_the_native_path_is_skipped() {
    // delivery.rs gates the native rich-blocks send on `!contains_table`; if
    // this is true, the message goes to the clean monospace-HTML path.
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
