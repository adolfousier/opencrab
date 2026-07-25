//! Model-picker position registry (#761).
//!
//! The registry is process-global, so every test uses its own provider key
//! rather than clearing shared state, which would make these order-dependent.

use crate::channels::model_menu::{remember, resolve_index};

fn models(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_string()).collect()
}

#[test]
fn resolves_a_position_to_the_model_that_occupied_it() {
    let list = models(&[
        "fast-1",
        "org/very-long-model-name-that-cannot-fit",
        "slow-3",
    ]);
    remember("test-resolve", &list);

    assert_eq!(resolve_index("test-resolve", 0).as_deref(), Some("fast-1"));
    assert_eq!(
        resolve_index("test-resolve", 1).as_deref(),
        Some("org/very-long-model-name-that-cannot-fit")
    );
    assert_eq!(resolve_index("test-resolve", 2).as_deref(), Some("slow-3"));
}

#[test]
fn unknown_provider_resolves_to_nothing() {
    // Lets the caller fall back to config rather than inventing a name.
    assert_eq!(resolve_index("test-never-rendered", 0), None);
}

#[test]
fn position_past_the_end_resolves_to_nothing() {
    remember("test-past-end", &models(&["only-one"]));
    assert_eq!(resolve_index("test-past-end", 1), None);
}

#[test]
fn a_new_render_replaces_the_previous_positions() {
    // The live inventory can grow between renders. A tapped button must map
    // to the menu that drew it, which is always the most recent one.
    remember("test-rerender", &models(&["old-a", "old-b"]));
    remember("test-rerender", &models(&["new-a", "new-b", "new-c"]));

    assert_eq!(resolve_index("test-rerender", 0).as_deref(), Some("new-a"));
    assert_eq!(resolve_index("test-rerender", 2).as_deref(), Some("new-c"));
}

#[test]
fn providers_do_not_share_positions() {
    remember("test-prov-x", &models(&["x-model"]));
    remember("test-prov-y", &models(&["y-model"]));

    assert_eq!(resolve_index("test-prov-x", 0).as_deref(), Some("x-model"));
    assert_eq!(resolve_index("test-prov-y", 0).as_deref(), Some("y-model"));
}
