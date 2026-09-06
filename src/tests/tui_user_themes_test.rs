use ratatui::style::Color;

use crate::tui::render::theme::rgb_to_ansi256;
use crate::tui::render::user_themes::*;

/// A minimal-but-complete valid preset (crab-dark values).
const VALID: &str = r##"
accent = "#D76414"
accent_teal = "#008080"
accent_soft = "#FFFFFF"
text_primary = "#C8C8D2"
text_secondary = "#8C8CA0"
text_muted = "#505064"
text_dim = "#3C3C50"
gray = "#646464"
gray_mid = "#646464"
gray_detail = "#787878"
gray_dim = "#505050"
gray_dark = "#282828"
gray_base = "#141414"
gray_light = "#C8C8C8"
gray_soft = "#A0A0B0"
gray_muted = "#808090"
success = "#50C878"
analytics_green = "#50C878"
green_check = "#50C878"
error = "#DC5050"
error_soft = "#DC4646"
error_faded = "#DC8282"
warning = "#E6B450"
warning_muted = "#C8AA3C"
amber_muted = "#EBA046"
teal_vivid = "#3FBFBF"
teal_bright = "#7FD4D4"
teal_muted = "#4A9E9E"
teal_calm = "#35777E"
blue_slate = "#4A5A7A"
blue_steel = "#5B7EA6"
blue_link = "#5AA0E6"
blue_sky = "#8FC5F0"
blue_soft = "#6E8CB8"
blue_vivid = "#4D9DE0"
blue_code = "#7D96C3"
selection_bg = "#2A3B5C"
surface_panel = "#1E1E2D"
surface_qr = "#121212"
surface_code = "#181820"
surface_code_alt = "#202028"
ink = "#14141E"
purple_soft = "#B39DDB"
"##;

#[test]
fn hex_parsing_accepts_both_forms() {
    assert_eq!(parse_hex("#FF8040"), Some(Color::Rgb(0xFF, 0x80, 0x40)));
    assert_eq!(parse_hex("ff8040"), Some(Color::Rgb(0xFF, 0x80, 0x40)));
    assert_eq!(parse_hex("#GG8040"), None);
    assert_eq!(parse_hex("#FF80"), None);
}

#[test]
fn contrast_math_matches_wcag_reference() {
    // Black on white = 21:1 exactly.
    assert!((contrast(Color::Rgb(0, 0, 0), Color::Rgb(255, 255, 255)) - 21.0).abs() < 1e-9);
    // Same color = 1:1.
    assert!((contrast(Color::Rgb(10, 20, 30), Color::Rgb(10, 20, 30)) - 1.0).abs() < 1e-9);
}

#[test]
fn crab_dark_reference_passes_contrast_floor() {
    let t = &crate::tui::render::theme::CRAB_DARK;
    for &(fg, bg) in CONTRAST_PAIRS {
        let r = contrast(t.colors.get(fg), t.colors.get(bg));
        assert!(
            r >= CONTRAST_FLOOR,
            "reference preset fails its own floor on {fg:?}/{bg:?}: {r:.2}"
        );
    }
}

#[test]
fn valid_preset_builds_and_passes_validation() {
    let t = build_theme("mine", VALID).expect("valid preset rejected");
    assert_eq!(t.name, "mine");
    assert_eq!(t.colors.accent, Color::Rgb(0xD7, 0x64, 0x14));
    // ANSI tier derived, not hand-written.
    assert_eq!(t.colors.ansi.accent, rgb_to_ansi256(0xD7, 0x64, 0x14));
}

#[test]
fn missing_role_is_rejected_with_field_name() {
    let broken = VALID.replace("purple_soft = \"#B39DDB\"\n", "");
    let err = build_theme("mine", &broken).unwrap_err();
    assert!(
        err.contains("purple_soft"),
        "error should name the field: {err}"
    );
}

#[test]
fn unknown_key_is_rejected() {
    let broken = format!("{VALID}\naccent2 = \"#000000\"\n");
    let err = build_theme("mine", &broken).unwrap_err();
    assert!(
        err.contains("accent2"),
        "error should name the unknown key: {err}"
    );
}

#[test]
fn bad_hex_is_rejected_with_role_name() {
    let broken = VALID.replace("ink = \"#14141E\"", "ink = \"nope\"");
    let err = build_theme("mine", &broken).unwrap_err();
    assert!(err.contains("ink"), "error should name the role: {err}");
}

#[test]
fn low_contrast_is_rejected_with_ratio() {
    // text_primary ~ ink → unreadable.
    let broken = VALID.replace("text_primary = \"#C8C8D2\"", "text_primary = \"#151520\"");
    let err = build_theme("mine", &broken).unwrap_err();
    assert!(err.contains("contrast"), "should cite contrast: {err}");
    assert!(err.contains("text_primary"), "should name the pair: {err}");
}

#[test]
fn built_in_name_collision_is_rejected() {
    let err = build_theme("dracula", VALID).unwrap_err();
    assert!(err.contains("collides"), "should reject collision: {err}");
}

#[test]
fn load_from_dir_sorts_and_reports_rejects() {
    let dir = std::env::temp_dir().join(format!("oc-themes-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("zeta.toml"), VALID).unwrap();
    std::fs::write(dir.join("alpha.toml"), VALID).unwrap();
    std::fs::write(dir.join("broken.toml"), "not toml at all: [").unwrap();

    let report = load_from(&dir);
    assert_eq!(report.themes.len(), 2);
    assert_eq!(report.themes[0].name, "alpha");
    assert_eq!(report.rejected.len(), 1);
    assert_eq!(report.rejected[0].file, "broken.toml");

    let _ = std::fs::remove_dir_all(&dir);
}
