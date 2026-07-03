use crate::config::{Config, ProviderConfig, ProviderConfigs};
use crate::utils::file_extract::*;

/// #279 regression: the video gate must accept a provider `vision_model`, not
/// only Gemini. Before the fix, `is_video_vision_available` returned true only
/// for `image.vision.enabled` + a Gemini key, so a non-Gemini provider with
/// image vision could see images but never videos or GIFs.
#[test]
fn video_gate_accepts_provider_vision_model_without_gemini() {
    let config = Config {
        providers: ProviderConfigs {
            openai: Some(ProviderConfig {
                enabled: true,
                api_key: Some("provider-key".to_string()),
                vision_model: Some("gpt-vision".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    // No Gemini key anywhere; image.vision stays disabled by default.
    assert!(!config.image.vision.enabled);
    assert!(
        is_video_vision_available(&config),
        "a provider vision_model must make video vision available (#279)"
    );
}

/// Symmetry guard: images already accepted a provider vision_model. Video must
/// now match, so the two gates agree on the same non-Gemini config.
#[test]
fn video_gate_matches_image_gate_for_provider_vision() {
    let config = Config {
        providers: ProviderConfigs {
            openai: Some(ProviderConfig {
                enabled: true,
                api_key: Some("provider-key".to_string()),
                vision_model: Some("gpt-vision".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(
        is_vision_available(&config),
        is_video_vision_available(&config),
        "video vision availability must mirror image vision availability"
    );
}

/// With no vision backend at all, the video gate stays closed.
#[test]
fn video_gate_closed_without_any_vision_backend() {
    let config = Config {
        providers: ProviderConfigs {
            openai: Some(ProviderConfig {
                enabled: true,
                api_key: Some("provider-key".to_string()),
                vision_model: None,
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(!is_video_vision_available(&config));
}

#[test]
fn collapse_double_extension_handles_double_docx() {
    assert_eq!(
        collapse_double_extension("zaiavlenie_s_ekzamena.docx.docx"),
        "zaiavlenie_s_ekzamena.docx"
    );
}

#[test]
fn collapse_double_extension_handles_case_insensitive() {
    assert_eq!(collapse_double_extension("file.DOCX.DOCX"), "file.DOCX");
    assert_eq!(collapse_double_extension("FILE.Pdf.Pdf"), "FILE.Pdf");
}

#[test]
fn collapse_double_extension_passthrough_no_double() {
    assert_eq!(collapse_double_extension("file.docx"), "file.docx");
    assert_eq!(collapse_double_extension("file.doc.pdf"), "file.doc.pdf");
    assert_eq!(collapse_double_extension("file"), "file");
}

#[test]
fn collapse_double_extension_short() {
    assert_eq!(collapse_double_extension("a.docx.docx"), "a.docx");
    assert_eq!(collapse_double_extension("a.b.c"), "a.b.c");
}

#[test]
fn collapse_double_extension_single_char_extension() {
    // Edge case: single-char inner ext same as outer (e.g. "file.x.x")
    assert_eq!(collapse_double_extension("file.x.x"), "file.x");
}
