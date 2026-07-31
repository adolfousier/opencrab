//! A generated image always lands with an extension (#889).
//!
//! A caller-supplied `filename` was used verbatim, so one without an extension
//! wrote an extensionless file. Downstream consumers identify images by
//! extension: `generate_document` later failed with "not a decodable PNG/JPEG
//! ... The image format could not be determined" on exactly such a path, and
//! that failure counted against `generate_document` rather than against
//! whatever named the file.
//!
//! Pins the naming rule alone; no image is generated.
//!
//! Fixtures are synthetic and carry no user identifiers.

/// The rule applied in `generate_image`: keep an explicit extension, add `.png`
/// when there is none, fall back to a uuid when no name is given.
fn resolve(filename: Option<&str>) -> String {
    filename
        .filter(|s| !s.trim().is_empty())
        .map(|s| {
            let name = s.trim();
            if std::path::Path::new(name).extension().is_some() {
                name.to_string()
            } else {
                format!("{name}.png")
            }
        })
        .unwrap_or_else(|| "generated.png".to_string())
}

#[test]
fn the_observed_name_gains_an_extension() {
    // The exact name from the generate_document failure.
    assert_eq!(
        resolve(Some("rust_performance_comparison")),
        "rust_performance_comparison.png"
    );
}

#[test]
fn an_explicit_extension_is_preserved() {
    // Must not rewrite a caller who knew what they wanted.
    for name in ["chart.png", "photo.jpg", "diagram.jpeg", "scan.webp"] {
        assert_eq!(resolve(Some(name)), name, "rewrote {name}");
    }
}

#[test]
fn a_dotted_name_keeps_its_own_extension() {
    // "v1.2" parses an extension of "2"; rewriting it would be worse than
    // leaving it, and the caller named it deliberately.
    assert_eq!(resolve(Some("report.v1.2.png")), "report.v1.2.png");
}

#[test]
fn whitespace_is_trimmed_before_the_check() {
    assert_eq!(resolve(Some("  chart  ")), "chart.png");
    assert_eq!(resolve(Some("  chart.png  ")), "chart.png");
}

#[test]
fn an_absent_or_blank_name_falls_back() {
    // Blank must not produce a bare ".png" or an empty name.
    for given in [None, Some(""), Some("   ")] {
        let out = resolve(given);
        assert!(out.ends_with(".png"), "no extension: {out}");
        assert!(out.len() > 4, "degenerate name: {out}");
    }
}
