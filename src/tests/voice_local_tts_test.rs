//! Tests for voice/local_tts.rs: sample rate extraction, Piper voices,
//! TTS text cleaning, PCM/WAV/OGG encoding.

use crate::channels::voice::local_tts::{
    PIPER_VOICES, clean_for_tts, encode_ogg_opus, extract_sample_rate, find_piper_voice, ogg_crc32,
    pcm_to_wav,
};

// ── extract_sample_rate ───────────────────────────────────────────────

#[test]
fn extract_sample_rate_present() {
    let config = r#"{"sample_rate": 22050, "other": "stuff"}"#;
    assert_eq!(extract_sample_rate(config), Some(22050));
}

#[test]
fn extract_sample_rate_missing() {
    let config = r#"{"other": "stuff"}"#;
    assert_eq!(extract_sample_rate(config), None);
}

// ── find_piper_voice ──────────────────────────────────────────────────

#[test]
fn find_piper_voice_known() {
    assert!(find_piper_voice("ryan").is_some());
    assert!(find_piper_voice("amy").is_some());
}

#[test]
fn find_piper_voice_unknown() {
    assert!(find_piper_voice("nonexistent").is_none());
}

#[test]
fn piper_voice_urls() {
    let ryan = find_piper_voice("ryan").unwrap();
    assert!(ryan.onnx_url().contains("en_US"));
    assert!(ryan.onnx_url().contains("ryan"));
    assert!(ryan.onnx_url().ends_with(".onnx"));
    assert!(ryan.config_url().ends_with(".onnx.json"));
}

// ── clean_for_tts ─────────────────────────────────────────────────────

#[test]
fn clean_for_tts_strips_markdown() {
    let input = "**Hello** *world*! Check `this_code` out.";
    let cleaned = clean_for_tts(input);
    assert_eq!(cleaned, "Hello world! Check this_code out.");
}

#[test]
fn clean_for_tts_keeps_code_block_content() {
    let input = "Here is code:\n\n```rust\nfn main() {}\n```\n\nDone.";
    let cleaned = clean_for_tts(input);
    assert!(cleaned.contains("fn main()"));
    assert!(cleaned.contains("Done."));
    assert!(!cleaned.contains("```"));
}

#[test]
fn clean_for_tts_collapses_whitespace() {
    let input = "Hello    world   how   are  you";
    let cleaned = clean_for_tts(input);
    assert_eq!(cleaned, "Hello world how are you");
}

#[test]
fn clean_for_tts_collapses_punctuation() {
    let input = "Wow!!! Really??? Yes...";
    let cleaned = clean_for_tts(input);
    assert_eq!(cleaned, "Wow! Really? Yes.");
}

#[test]
fn clean_for_tts_strips_headers() {
    let input = "## My Header\nSome text";
    let cleaned = clean_for_tts(input);
    assert_eq!(cleaned, "My Header. Some text");
}

#[test]
fn clean_for_tts_strips_bullets() {
    let input = "- First item\n- Second item";
    let cleaned = clean_for_tts(input);
    assert_eq!(cleaned, "First item. Second item");
}

// ── default voice ─────────────────────────────────────────────────────

#[test]
fn default_voice_is_ryan() {
    assert_eq!(PIPER_VOICES[0].id, "ryan");
}

// ── pcm_to_wav ────────────────────────────────────────────────────────

#[test]
fn pcm_to_wav_header() {
    let samples = vec![0i16, 100, -100, 32767, -32768];
    let wav = pcm_to_wav(&samples, 22050).unwrap();
    assert_eq!(&wav[..4], b"RIFF");
    assert_eq!(&wav[8..12], b"WAVE");
}

// ── encode_ogg_opus ──────────────────────────────────────────────────

#[test]
fn encode_ogg_opus_produces_ogg() {
    let samples = vec![0i16; 960];
    let ogg = encode_ogg_opus(&samples, 48000).unwrap();
    assert_eq!(&ogg[..4], b"OggS", "Should produce OGG container");
}

// ── ogg_crc32 ────────────────────────────────────────────────────────

#[test]
fn ogg_crc32_empty() {
    assert_eq!(ogg_crc32(&[]), 0);
}
