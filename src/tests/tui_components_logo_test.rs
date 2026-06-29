use crate::tui::components::logo::*;

#[test]
fn test_logo_not_empty() {
    assert!(!get_logo().is_empty());
    assert!(!get_croissant().is_empty());
    assert!(!get_small_logo().is_empty());
}

#[test]
fn test_logo_with_version() {
    let logo = get_logo_with_version("0.1.0");
    assert!(logo.contains("0.1.0"));
}
