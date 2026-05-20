//! Tests for voice/local_whisper.rs: transcript cleaning, PcmSource,
//! audio decoding, resampling, model presets.

use crate::channels::voice::local_whisper::{
    DownloadProgress, LOCAL_MODEL_PRESETS, PcmSource, clean_transcript, decode_audio,
    find_local_model, is_model_downloaded, model_path, parse_whisper_source, resample,
};

// ── clean_transcript ──────────────────────────────────────────────────

#[test]
fn clean_transcript_collapses_whitespace() {
    assert_eq!(clean_transcript("  hello   world  "), "hello world");
}

#[test]
fn clean_transcript_handles_newlines_and_tabs() {
    assert_eq!(clean_transcript("hello\n\tworld\n"), "hello world");
}

#[test]
fn clean_transcript_empty_input() {
    assert_eq!(clean_transcript(""), "");
    assert_eq!(clean_transcript("   "), "");
}

// ── PcmSource (rodio::Source impl) ────────────────────────────────────

#[test]
fn pcm_source_iterates_all_samples() {
    use std::iter::Iterator;
    let samples = vec![0.1, 0.2, 0.3, 0.4, 0.5];
    let mut source = PcmSource::new(samples.clone(), 16000);
    let collected: Vec<f32> = std::iter::from_fn(|| source.next()).collect();
    assert_eq!(collected, samples);
}

#[test]
fn pcm_source_empty() {
    use std::iter::Iterator;
    let mut source = PcmSource::new(vec![], 16000);
    assert!(source.next().is_none());
}

#[test]
fn pcm_source_rodio_metadata() {
    use rodio::Source;
    let source = PcmSource::new(vec![0.0; 16000], 16000);
    assert_eq!(source.channels(), 1);
    assert_eq!(source.sample_rate(), 16000);
    assert_eq!(
        source.total_duration(),
        Some(std::time::Duration::from_secs(1))
    );
    assert_eq!(source.current_frame_len(), Some(16000));
}

#[test]
fn pcm_source_frame_len_decreases() {
    use rodio::Source;
    let mut source = PcmSource::new(vec![0.0; 10], 16000);
    assert_eq!(source.current_frame_len(), Some(10));
    use std::iter::Iterator;
    source.next();
    assert_eq!(source.current_frame_len(), Some(9));
}

// ── parse_whisper_source ──────────────────────────────────────────────

#[test]
fn parse_all_preset_sources() {
    for preset in LOCAL_MODEL_PRESETS {
        let result = parse_whisper_source(preset);
        assert!(
            result.is_ok(),
            "Failed to parse source for preset '{}': {:?}",
            preset.id,
            result.err()
        );
    }
}

#[test]
fn parse_unknown_source_fails() {
    use crate::channels::voice::local_whisper::LocalModelPreset;
    let fake = LocalModelPreset {
        id: "fake",
        label: "Fake",
        file_name: "fake",
        size_label: "0",
        repo_id: "NonExistent",
    };
    let result = parse_whisper_source(&fake);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Unknown whisper source")
    );
}

// ── is_model_downloaded ──────────────────────────────────────────────

#[test]
fn is_model_downloaded_always_true() {
    for preset in LOCAL_MODEL_PRESETS {
        assert!(
            is_model_downloaded(preset),
            "is_model_downloaded should always be true for rwhisper presets"
        );
    }
}

// ── decode_audio ──────────────────────────────────────────────────────

#[test]
fn decode_audio_empty_fails() {
    let result = decode_audio(&[]);
    assert!(result.is_err());
}

#[test]
fn decode_audio_garbage_fails() {
    let result = decode_audio(&[0xFF, 0xFE, 0xFD, 0xFC, 0xFB]);
    assert!(result.is_err());
}

#[test]
fn decode_wav_valid_sine() {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut buf = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut buf);
        let mut writer = hound::WavWriter::new(cursor, spec).unwrap();
        for i in 0..1600 {
            let t = i as f32 / 16000.0;
            let sample = (t * 440.0 * 2.0 * std::f32::consts::PI).sin();
            writer
                .write_sample((sample * i16::MAX as f32) as i16)
                .unwrap();
        }
        writer.finalize().unwrap();
    }

    let (samples, rate) = decode_audio(&buf).unwrap();
    assert_eq!(rate, 16000);
    assert_eq!(samples.len(), 1600);
    assert!(samples[0].abs() < 0.01);
}

#[test]
fn decode_wav_stereo_mixes_to_mono() {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 16000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut buf = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut buf);
        let mut writer = hound::WavWriter::new(cursor, spec).unwrap();
        for _ in 0..100 {
            writer.write_sample(1000i16).unwrap();
            writer.write_sample(3000i16).unwrap();
        }
        writer.finalize().unwrap();
    }

    let (samples, rate) = decode_audio(&buf).unwrap();
    assert_eq!(rate, 16000);
    assert_eq!(samples.len(), 100);
    let expected = 2000.0 / i16::MAX as f32;
    assert!((samples[0] - expected).abs() < 0.001);
}

// ── resample ──────────────────────────────────────────────────────────

#[test]
fn resample_48k_to_16k() {
    let input: Vec<f32> = (0..9600)
        .map(|i| (i as f32 / 48000.0 * 440.0 * 2.0 * std::f32::consts::PI).sin())
        .collect();
    let output = resample(&input, 48000, 16000).unwrap();
    let expected_len = (input.len() as f64 * 16000.0 / 48000.0) as usize;
    assert!(
        (output.len() as i64 - expected_len as i64).unsigned_abs() < 256,
        "Expected ~{} samples, got {}",
        expected_len,
        output.len()
    );
}

#[test]
fn resample_preserves_non_silence() {
    let input: Vec<f32> = (0..4800)
        .map(|i| (i as f32 / 48000.0 * 440.0 * 2.0 * std::f32::consts::PI).sin())
        .collect();
    let output = resample(&input, 48000, 16000).unwrap();
    let rms: f32 = (output.iter().map(|s| s * s).sum::<f32>() / output.len() as f32).sqrt();
    assert!(
        rms > 0.1,
        "Resampled audio should not be silence, RMS={}",
        rms
    );
}

// ── DownloadProgress ──────────────────────────────────────────────────

#[test]
fn download_progress_done_state() {
    let p = DownloadProgress {
        downloaded: 42_000_000,
        total: Some(42_000_000),
        done: true,
        error: None,
    };
    assert!(p.done);
    assert_eq!(p.downloaded, p.total.unwrap());
    assert!(p.error.is_none());
}

#[test]
fn download_progress_error_state() {
    let p = DownloadProgress {
        downloaded: 0,
        total: None,
        done: false,
        error: Some("network timeout".to_string()),
    };
    assert!(!p.done);
    assert!(p.error.is_some());
}

// ── Audio sanitization (logic tests) ──────────────────────────────────

#[test]
fn sanitize_nan_inf_in_audio() {
    let mut samples = vec![0.5, f32::NAN, -0.3, f32::INFINITY, f32::NEG_INFINITY, 0.1];
    for s in samples.iter_mut() {
        if !s.is_finite() {
            *s = 0.0;
        }
    }
    assert_eq!(samples, vec![0.5, 0.0, -0.3, 0.0, 0.0, 0.1]);
}

#[test]
fn short_audio_padded_to_minimum() {
    const MIN_SAMPLES: usize = 16000;
    let mut audio = vec![0.5f32; 100];
    if audio.len() < MIN_SAMPLES {
        audio.resize(MIN_SAMPLES, 0.0);
    }
    assert_eq!(audio.len(), MIN_SAMPLES);
    assert_eq!(audio[0], 0.5);
    assert_eq!(audio[100], 0.0);
    assert_eq!(audio[15999], 0.0);
}

#[test]
fn audio_at_minimum_not_padded() {
    const MIN_SAMPLES: usize = 16000;
    let mut audio = vec![0.1f32; MIN_SAMPLES];
    let original_len = audio.len();
    if audio.len() < MIN_SAMPLES {
        audio.resize(MIN_SAMPLES, 0.0);
    }
    assert_eq!(audio.len(), original_len);
}

#[test]
fn audio_above_minimum_not_padded() {
    const MIN_SAMPLES: usize = 16000;
    let mut audio = vec![0.1f32; 48000];
    let original_len = audio.len();
    if audio.len() < MIN_SAMPLES {
        audio.resize(MIN_SAMPLES, 0.0);
    }
    assert_eq!(audio.len(), original_len);
}

// ── Model presets ─────────────────────────────────────────────────────

#[test]
fn default_preset_is_quantized_tiny() {
    let preset = find_local_model("local-tiny").unwrap();
    assert_eq!(preset.repo_id, "QuantizedTiny");
    assert!(preset.label.contains("Multilingual"));
    assert!(preset.label.contains("Quantized"));
}

#[test]
fn all_presets_have_unique_ids() {
    let mut ids: Vec<&str> = LOCAL_MODEL_PRESETS.iter().map(|p| p.id).collect();
    let len = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), len, "Preset IDs must be unique");
}

#[test]
fn model_path_under_opencrabs_dir() {
    let preset = &LOCAL_MODEL_PRESETS[0];
    let path = model_path(preset);
    let path_str = path.to_string_lossy();
    assert!(
        path_str.contains("opencrabs"),
        "Path should be under opencrabs dir"
    );
    assert!(
        path_str.contains("whisper"),
        "Path should be under whisper subdir"
    );
    assert!(path_str.ends_with(preset.file_name));
}
