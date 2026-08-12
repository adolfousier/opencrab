//! Tests for the `browser_find` inventory-mode JS builder
//! (`build_inventory_js`), used when the agent calls `browser_find`
//! with no `pattern` to enumerate every visible interactive element
//! on the page (#1022). We pin the shape of the JS we send into the
//! page so the selectors the model passes back to `browser_click` are
//! deterministic and identical in shape to the search-mode results.
//!
//! We can't run the JS (that requires a real page / V8), so these
//! tests verify we emit the right selector union, pre-filter to
//! visible elements, respect the limit, and reuse the shared
//! `{selector, text, tag, visible}` serialization.

#![cfg(feature = "browser")]

use crate::brain::tools::browser::build_inventory_js;

#[test]
fn inventory_targets_interactive_element_union() {
    // The inventory must enumerate the standard "interactive" set, not
    // every element on the page. Each of these MUST appear so a click
    // target is never silently dropped.
    let js = build_inventory_js(50);
    assert!(js.contains("a[href]"), "links");
    assert!(js.contains("button"), "buttons");
    assert!(
        js.contains("input:not([type=\"hidden\"])"),
        "visible inputs"
    );
    assert!(js.contains("select"), "selects");
    assert!(js.contains("textarea"), "textareas");
    assert!(js.contains("summary"), "disclosure summaries");
    assert!(js.contains("[role=\"button\"]"), "ARIA buttons");
    assert!(js.contains("[role=\"link\"]"), "ARIA links");
    assert!(js.contains("[role=\"checkbox\"]"), "ARIA checkboxes");
    assert!(js.contains("[role=\"tab\"]"), "ARIA tabs");
    assert!(js.contains("[role=\"menuitem\"]"), "ARIA menu items");
    assert!(js.contains("[role=\"option\"]"), "ARIA options");
    assert!(js.contains("[contenteditable=\"true\"]"), "contenteditable");
    // tabindex excludes -1 (not focusable) but includes the bare attribute.
    assert!(js.contains("[tabindex]:not([tabindex=\"-1\"])"), "tabindex");
}

#[test]
fn inventory_pre_filters_to_visible_elements() {
    // Off-screen / hidden elements waste the index and can never be
    // clicked, so the inventory must drop them at collection time,
    // BEFORE indexing. Mirrors the visibility check in the shared
    // serializer but applied earlier to keep the index dense.
    let js = build_inventory_js(50);
    assert!(js.contains("getBoundingClientRect()"));
    assert!(js.contains("rect.width > 0"));
    assert!(js.contains("rect.height > 0"));
    assert!(js.contains("getComputedStyle(el).visibility !== 'hidden'"));
    assert!(js.contains("getComputedStyle(el).display !== 'none'"));
}

#[test]
fn inventory_respects_the_limit() {
    // The cap bounds the collection so a page with 800 buttons does not
    // flood context. The limit is enforced inside the collection loop,
    // so the index never exceeds it even though the serializer also
    // walks the full node array.
    let js = build_inventory_js(40);
    assert!(js.contains("visible.length >= 40"));
    assert!(js.contains("break"));
}

#[test]
fn inventory_uses_shared_match_index_serializer() {
    // Inventory and search results MUST be serialized identically so the
    // model sees one shape regardless of how the nodes were collected.
    // The shared `wrap_with_index` step clears stale attributes, stamps a
    // stable per-index `data-opencrabs-match`, and returns the same
    // `{selector, text, tag, visible}` tuple as search mode.
    let js = build_inventory_js(50);
    assert!(
        js.contains("removeAttribute('data-opencrabs-match')"),
        "must clear stale match attributes before re-indexing"
    );
    assert!(
        js.contains(r#"selector: '[data-opencrabs-match="' + i + '"]'"#),
        "must return the stable indexed selector shape"
    );
    assert!(js.contains("text:"));
    assert!(js.contains("tag:"));
    assert!(js.contains("visible:"));
}

#[test]
fn inventory_has_no_user_supplied_string_to_escape() {
    // The selector union is a fixed string with no user input, so unlike
    // search mode there is no injection surface. Sanity-check that the
    // union is a single static querySelectorAll argument and contains no
    // format placeholders from the caller.
    let js = build_inventory_js(10);
    // The limit is the only interpolation; the selector union is literal.
    assert!(
        !js.contains(r#""+ "#),
        "no concatenation into the selector string"
    );
    assert!(js.contains("querySelectorAll(sel)"));
}
