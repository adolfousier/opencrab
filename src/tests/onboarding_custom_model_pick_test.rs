//! An exactly-typed model id wins over a substring hit (#873).
//!
//! The custom-provider picker filters with `contains`, then commits
//! `filtered[selected_model]`. Against a large catalogue a typed id is usually
//! also a substring of some longer entry, so the highlighted row won — and the
//! wizard silently wrote a DIFFERENT model than the one typed, with nothing on
//! screen to show it.
//!
//! This pins the resolution order alone; it does not need the TUI. The rule is:
//! an exact catalogue entry typed in full beats any substring match.
//!
//! Fixtures are synthetic and carry no user identifiers.

/// The resolution the Enter handler performs, extracted as data so the order
/// can be asserted without driving the widget.
fn resolve(models: &[&str], typed: &str, selected: usize) -> String {
    let filter = typed.to_lowercase();
    let filtered: Vec<&&str> = models
        .iter()
        .filter(|m| m.to_lowercase().contains(&filter))
        .collect();
    let typed_raw = typed.trim();
    let typed_is_exact = !typed_raw.is_empty() && models.contains(&typed_raw);
    if typed_is_exact {
        typed_raw.to_string()
    } else if let Some(m) = filtered.get(selected) {
        (**m).to_string()
    } else if !typed_raw.is_empty() {
        typed_raw.to_string()
    } else {
        String::new()
    }
}

/// A real shape: the short id is a prefix of two longer dated variants.
const CATALOGUE: &[&str] = &[
    "qwen3.7-flash-2026-07-15",
    "qwen3.7-flash",
    "qwen3.7-flash-preview",
    "kimi-k2.7-code",
];

#[test]
fn an_exact_typed_id_is_not_overridden_by_a_longer_variant() {
    // The silent case: "qwen3.7-flash" is a substring of the dated build, which
    // sorts first, so the old order committed the wrong model.
    assert_eq!(resolve(CATALOGUE, "qwen3.7-flash", 0), "qwen3.7-flash");
}

#[test]
fn a_partial_id_still_selects_the_highlighted_row() {
    // Browsing must keep working: an incomplete filter is a filter, not an id.
    assert_eq!(resolve(CATALOGUE, "flash", 0), "qwen3.7-flash-2026-07-15");
    assert_eq!(resolve(CATALOGUE, "flash", 2), "qwen3.7-flash-preview");
}

#[test]
fn an_id_absent_from_the_catalogue_is_taken_as_typed() {
    // The escape hatch the picker exists for: a model /v1/models does not list.
    assert_eq!(
        resolve(CATALOGUE, "qwen3.8-max-preview", 0),
        "qwen3.8-max-preview"
    );
}

#[test]
fn an_empty_filter_selects_the_highlighted_row() {
    assert_eq!(resolve(CATALOGUE, "", 1), "qwen3.7-flash");
}

#[test]
fn an_exact_match_wins_from_any_cursor_position() {
    // The fix must not depend on where the cursor happens to sit.
    for selected in 0..CATALOGUE.len() {
        assert_eq!(
            resolve(CATALOGUE, "qwen3.7-flash", selected),
            "qwen3.7-flash",
            "cursor at {selected} overrode an exact match"
        );
    }
}

#[test]
fn surrounding_whitespace_does_not_defeat_an_exact_match() {
    assert_eq!(resolve(CATALOGUE, "  qwen3.7-flash  ", 0), "qwen3.7-flash");
}
