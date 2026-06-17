//! Tests for the channel-capable `/onboard:*` handlers — the routing, menus,
//! and argument validation. Config-writing paths touch real config files and
//! are exercised manually; here we cover the pure dispatch/guidance surface.

use crate::brain::tools::slash_onboard::{
    dispatch, onboard_channels, onboard_image, onboard_voice,
};

#[test]
fn unknown_step_errors() {
    let r = dispatch("frobnicate", "");
    let r = r.unwrap();
    assert!(!r.success);
    assert!(r.error.unwrap().contains("image, voice, channels"));
}

#[test]
fn brain_step_is_tui_only_message() {
    let r = dispatch("brain", "").unwrap();
    assert!(r.success);
    assert!(r.output.to_lowercase().contains("tui-only"));
}

#[test]
fn image_menu_lists_gemini_and_provider() {
    let r = onboard_image("").unwrap();
    assert!(r.success);
    assert!(r.output.contains("gemini"));
    assert!(r.output.contains("provider"));
}

#[test]
fn image_gemini_without_key_errors() {
    let r = onboard_image("gemini").unwrap();
    assert!(!r.success);
    assert!(r.error.unwrap().to_lowercase().contains("key"));
}

#[test]
fn image_provider_without_model_errors() {
    let r = onboard_image("provider").unwrap();
    assert!(!r.success);
    assert!(r.error.unwrap().to_lowercase().contains("model"));
}

#[test]
fn image_unknown_option_errors_with_menu() {
    let r = onboard_image("bogus xyz").unwrap();
    assert!(!r.success);
    let e = r.error.unwrap();
    assert!(e.contains("gemini") && e.contains("provider"));
}

#[test]
fn voice_menu_and_subcommand_validation() {
    assert!(onboard_voice("").unwrap().success);
    // stt groq needs a key
    let r = onboard_voice("stt groq").unwrap();
    assert!(!r.success);
    // unknown stt provider
    let r = onboard_voice("stt nope").unwrap();
    assert!(!r.success);
    assert!(r.error.unwrap().contains("groq"));
}

#[test]
fn channels_menu_and_missing_token() {
    assert!(onboard_channels("").unwrap().success);
    let r = onboard_channels("telegram").unwrap();
    assert!(!r.success);
    assert!(r.error.unwrap().to_lowercase().contains("token"));
}

#[test]
fn channels_whatsapp_points_at_qr_pairing() {
    let r = onboard_channels("whatsapp").unwrap();
    assert!(r.success);
    assert!(r.output.to_lowercase().contains("qr"));
}
