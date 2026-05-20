//! Voice Processing Module
//!
//! Speech-to-text (Groq Whisper) and text-to-speech (OpenAI TTS) services
//! used by the Telegram bot for voice note support.

use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;

use crate::config::VoiceConfig;

const GROQ_TRANSCRIPTION_URL: &str = "https://api.groq.com/openai/v1/audio/transcriptions";
const OPENAI_SPEECH_URL: &str = "https://api.openai.com/v1/audio/speech";

/// Transcribe audio bytes using Groq Whisper (whisper-large-v3-turbo).
///
/// Accepts OGG/Opus audio (Telegram voice note format).
/// Returns the transcribed text.
pub async fn transcribe_audio(audio_bytes: Vec<u8>, groq_api_key: &str) -> Result<String> {
    transcribe_audio_with_url(audio_bytes, groq_api_key, GROQ_TRANSCRIPTION_URL).await
}

/// Dispatch STT transcription based on config.
///
/// - Groq Whisper API (stt_provider)
/// - OpenAI-compatible STT (stt_base_url + stt_model)
/// - Voicebox STT (voicebox_stt_enabled)
/// - Local whisper (local-stt feature)
pub async fn transcribe(audio_bytes: Vec<u8>, voice_config: &VoiceConfig) -> Result<String> {
    // Voicebox STT takes priority if enabled
    if voice_config.voicebox_stt_enabled {
        return super::voicebox_stt::transcribe(audio_bytes, &voice_config.voicebox_stt_base_url)
            .await;
    }

    // OpenAI-compatible STT if base_url is configured
    if let (Some(base_url), Some(model)) = (&voice_config.stt_base_url, &voice_config.stt_model)
        && let Some(api_key) = &voice_config.stt_api_key
    {
        return super::openai_stt::transcribe_audio(audio_bytes, api_key, model, base_url).await;
    }

    // Groq API (legacy)
    if let Some(provider) = &voice_config.stt_provider
        && let Some(api_key) = &provider.api_key
    {
        return transcribe_audio(audio_bytes, api_key).await;
    }

    // Local whisper
    #[cfg(feature = "local-stt")]
    {
        if !super::local_stt_available() {
            anyhow::bail!(
                "Local STT is not supported on this CPU — AVX2 is required. \
                 Please switch to API STT in settings."
            );
        }
        let model_id = &voice_config.local_stt_model;
        let preset = super::local_whisper::find_local_model(model_id)
            .ok_or_else(|| anyhow::anyhow!("Unknown local STT model: {}", model_id))?;
        if !super::local_whisper::is_model_downloaded(preset) {
            tracing::info!(
                "Local STT model '{}' not in candle format — downloading automatically",
                model_id
            );
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let preset_id = model_id.to_string();
            let download_preset = super::local_whisper::find_local_model(&preset_id)
                .ok_or_else(|| anyhow::anyhow!("Unknown model: {}", preset_id))?;
            let download_handle = tokio::spawn(async move {
                super::local_whisper::download_model(download_preset, tx).await
            });
            while let Some(progress) = rx.recv().await {
                if progress.done {
                    break;
                }
                if let Some(err) = progress.error {
                    anyhow::bail!("Model download failed: {}", err);
                }
            }
            download_handle
                .await
                .map_err(|e| anyhow::anyhow!("Download task failed: {}", e))??;
            tracing::info!("Local STT model '{}' downloaded successfully", model_id);
        }
        tracing::info!("Local STT: transcribing with model {}", model_id);
        transcribe_audio_local(audio_bytes, model_id.clone()).await
    }

    #[cfg(not(feature = "local-stt"))]
    anyhow::bail!("No STT provider configured")
}

/// Internal: transcribe with configurable URL (for testing).
pub(crate) async fn transcribe_audio_with_url(
    audio_bytes: Vec<u8>,
    api_key: &str,
    url: &str,
) -> Result<String> {
    let client = Client::new();

    let file_part = reqwest::multipart::Part::bytes(audio_bytes)
        .file_name("voice.ogg")
        .mime_str("audio/ogg")?;

    let form = reqwest::multipart::Form::new()
        .part("file", file_part)
        .text("model", "whisper-large-v3-turbo")
        .text("response_format", "json");

    let response = client
        .post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .multipart(form)
        .send()
        .await
        .context("Failed to send audio to Groq Whisper")?;

    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_default();
        anyhow::bail!("Groq STT error ({}): {}", status, error_text);
    }

    let result: TranscriptionResponse = response
        .json()
        .await
        .context("Failed to parse Groq transcription response")?;

    tracing::info!("Groq STT: transcribed {} chars", result.text.len());

    Ok(result.text)
}

/// Dispatch TTS synthesis based on config.
///
/// - Voicebox TTS (voicebox_tts_enabled)
/// - OpenAI-compatible TTS (tts_base_url + tts_model)
/// - OpenAI TTS API (tts_provider)
/// - Local Piper TTS (local-tts feature)
///
/// All audio is converted to OGG/Opus before returning, since Telegram's
/// `send_voice` only accepts Opus format.
pub async fn synthesize(text: &str, voice_config: &VoiceConfig) -> Result<Vec<u8>> {
    if text.is_empty() {
        anyhow::bail!("Cannot synthesize empty text");
    }

    let audio = if voice_config.voicebox_tts_enabled {
        tracing::info!(
            "TTS dispatch → Voicebox (base_url={}, profile_id={}, engine={})",
            voice_config.voicebox_tts_base_url,
            voice_config.voicebox_tts_profile_id,
            voice_config.voicebox_tts_engine
        );
        let client = super::voicebox_tts::VoiceboxTts::new(
            &voice_config.voicebox_tts_base_url,
            &voice_config.voicebox_tts_profile_id,
            &voice_config.voicebox_tts_engine,
        );
        client.synthesize(text).await?
    } else if let (Some(base_url), Some(api_key)) =
        (&voice_config.tts_base_url, &voice_config.tts_api_key)
    {
        tracing::info!(
            "TTS dispatch → OpenAI-compatible (base_url={}, model={}, voice={})",
            base_url,
            voice_config.tts_model,
            voice_config.tts_voice
        );
        super::openai_tts::synthesize_speech(
            text,
            api_key,
            &voice_config.tts_voice,
            &voice_config.tts_model,
            base_url,
        )
        .await?
    } else if let Some(provider) = &voice_config.tts_provider
        && let Some(api_key) = &provider.api_key
    {
        tracing::info!(
            "TTS dispatch → OpenAI (model={}, voice={})",
            voice_config.tts_model,
            voice_config.tts_voice
        );
        synthesize_speech(
            text,
            api_key,
            &voice_config.tts_voice,
            &voice_config.tts_model,
        )
        .await?
    } else {
        #[cfg(feature = "local-tts")]
        {
            let voice_id = voice_config.local_tts_voice.clone();
            tracing::info!("TTS dispatch → Local Piper (voice={})", voice_id);
            synthesize_speech_local(text, &voice_id).await?
        }
        #[cfg(not(feature = "local-tts"))]
        anyhow::bail!("No TTS provider configured")
    };

    // Ensure Opus format for Telegram — converts WAV/MP3 if needed
    Ok(ensure_opus(audio).await)
}

/// Detect if audio bytes are already in OGG/Opus format.
fn is_opus(audio: &[u8]) -> bool {
    audio.starts_with(b"OggS")
}

/// Convert audio bytes to OGG/Opus format if needed.
///
/// Checks magic bytes to detect format. If already OGG/Opus, returns as-is.
/// Otherwise uses ffmpeg to convert with a 5-minute timeout.
/// Two-stage fallback: voip-tuned → basic opus → original bytes.
async fn ensure_opus(audio_bytes: Vec<u8>) -> Vec<u8> {
    if is_opus(&audio_bytes) {
        return audio_bytes;
    }

    let audio_len = audio_bytes.len();
    let audio_fallback = audio_bytes.clone();
    let handle = tokio::task::spawn_blocking(move || {
        let ffmpeg_path = std::env::var("FFMPEG_PATH").unwrap_or_else(|_| "ffmpeg".to_string());

        // Stage 1: voip-tuned opus (optimized for voice notes)
        let mut cmd = std::process::Command::new(&ffmpeg_path);
        cmd.args([
            "-i",
            "pipe:0",
            "-f",
            "ogg",
            "-c:a",
            "libopus",
            "-b:a",
            "48k",
            "-application",
            "voip",
            "-y",
            "pipe:1",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

        if let Ok(mut child) = cmd.spawn() {
            use std::io::Write;
            let write_ok = child
                .stdin
                .take()
                .map(|mut stdin| stdin.write_all(&audio_bytes).is_ok())
                .unwrap_or(false);

            if write_ok {
                if let Ok(output) = child.wait_with_output() {
                    if output.status.success() && !output.stdout.is_empty() {
                        tracing::info!(
                            "TTS: voip opus conversion ok ({} → {} bytes)",
                            audio_bytes.len(),
                            output.stdout.len()
                        );
                        return output.stdout;
                    }
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    tracing::warn!(
                        "TTS: voip opus failed ({}): {}",
                        output.status,
                        stderr.lines().next().unwrap_or("")
                    );
                }
            } else {
                tracing::warn!("TTS: failed to write audio to ffmpeg stage 1");
            }
        }

        // Stage 2: basic opus fallback (no voip tuning, higher bitrate)
        tracing::info!("TTS: trying basic opus fallback");
        let mut cmd2 = std::process::Command::new(&ffmpeg_path);
        cmd2.args([
            "-i", "pipe:0", "-f", "ogg", "-c:a", "libopus", "-b:a", "64k", "-y", "pipe:1",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

        if let Ok(mut child) = cmd2.spawn() {
            use std::io::Write;
            let write_ok = child
                .stdin
                .take()
                .map(|mut stdin| stdin.write_all(&audio_bytes).is_ok())
                .unwrap_or(false);

            if write_ok {
                if let Ok(output) = child.wait_with_output() {
                    if output.status.success() && !output.stdout.is_empty() {
                        tracing::info!(
                            "TTS: basic opus fallback ok ({} → {} bytes)",
                            audio_bytes.len(),
                            output.stdout.len()
                        );
                        return output.stdout;
                    }
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    tracing::warn!(
                        "TTS: basic opus fallback failed ({}): {}",
                        output.status,
                        stderr.lines().next().unwrap_or("")
                    );
                }
            } else {
                tracing::warn!("TTS: failed to write audio to ffmpeg stage 2");
            }
        } else {
            tracing::warn!("TTS: ffmpeg not found at '{}'", ffmpeg_path);
        }

        // Both stages failed — return original
        audio_bytes
    });

    match tokio::time::timeout(std::time::Duration::from_secs(300), handle).await {
        Ok(result) => result.unwrap_or(audio_fallback),
        Err(_) => {
            tracing::warn!(
                "TTS: ffmpeg conversion timed out after 5m ({} bytes), sending original audio",
                audio_len
            );
            audio_fallback
        }
    }
}

/// Synthesize speech using local Piper TTS. Runs in a blocking thread.
#[cfg(feature = "local-tts")]
async fn synthesize_speech_local(text: &str, voice_id: &str) -> Result<Vec<u8>> {
    let text = text.to_string();
    let voice_id = voice_id.to_string();

    let handle = tokio::task::spawn_blocking(move || {
        let engine = super::local_tts::PiperTts::new(&voice_id)?;
        engine.synthesize_opus(&text)
    });

    match tokio::time::timeout(std::time::Duration::from_secs(120), handle).await {
        Ok(Ok(result)) => result,
        Ok(Err(e)) => anyhow::bail!("Local TTS task panicked: {}", e),
        Err(_) => anyhow::bail!("Local TTS timed out after 120s"),
    }
}

/// Synthesize speech from text using OpenAI TTS.
///
/// Returns OGG/Opus audio bytes suitable for Telegram voice notes.
pub async fn synthesize_speech(
    text: &str,
    openai_api_key: &str,
    voice: &str,
    model: &str,
) -> Result<Vec<u8>> {
    synthesize_speech_with_url(text, openai_api_key, voice, model, OPENAI_SPEECH_URL).await
}

/// Internal: synthesize with configurable URL (for testing).
pub(crate) async fn synthesize_speech_with_url(
    text: &str,
    api_key: &str,
    voice: &str,
    model: &str,
    url: &str,
) -> Result<Vec<u8>> {
    let client = Client::new();

    let body = serde_json::json!({
        "model": model,
        "input": text,
        "voice": voice,
        "response_format": "opus",
    });

    let response = client
        .post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .context("Failed to send TTS request to OpenAI")?;

    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_default();
        anyhow::bail!("OpenAI TTS error ({}): {}", status, error_text);
    }

    let audio_bytes = response
        .bytes()
        .await
        .context("Failed to read TTS audio bytes")?
        .to_vec();

    tracing::info!(
        "OpenAI TTS: generated {} bytes of audio (voice={}, model={})",
        audio_bytes.len(),
        voice,
        model,
    );

    Ok(audio_bytes)
}

#[derive(Deserialize)]
pub(crate) struct TranscriptionResponse {
    pub(crate) text: String,
}

/// Cached local whisper model — loaded once, reused across transcriptions.
#[cfg(feature = "local-stt")]
static CACHED_WHISPER: tokio::sync::OnceCell<super::local_whisper::LocalWhisper> =
    tokio::sync::OnceCell::const_new();

/// Preload the local whisper model into the cache in the background.
/// Called at startup when local STT is configured so the first voice message is fast.
#[cfg(feature = "local-stt")]
pub async fn preload_local_whisper(model_id: &str) -> Result<()> {
    CACHED_WHISPER
        .get_or_try_init(|| async {
            let preset = super::local_whisper::find_local_model(model_id)
                .ok_or_else(|| anyhow::anyhow!("Unknown local STT model: {}", model_id))?;
            super::local_whisper::LocalWhisper::new(preset).await
        })
        .await?;
    Ok(())
}

/// Transcribe audio bytes using a local whisper model (rwhisper).
///
/// The model is loaded once and cached for subsequent calls.
#[cfg(feature = "local-stt")]
pub async fn transcribe_audio_local(audio_bytes: Vec<u8>, model_id: String) -> Result<String> {
    let len = audio_bytes.len();
    tracing::info!(
        "Local STT: starting transcription ({} bytes audio, model {})",
        len,
        model_id
    );

    let whisper = CACHED_WHISPER
        .get_or_try_init(|| async {
            let preset = super::local_whisper::find_local_model(&model_id)
                .ok_or_else(|| anyhow::anyhow!("Unknown local STT model: {}", model_id))?;
            super::local_whisper::LocalWhisper::new(preset).await
        })
        .await?;

    // Generous timeout — CPU transcription of long audio can take minutes
    match tokio::time::timeout(
        std::time::Duration::from_secs(300),
        whisper.transcribe(&audio_bytes),
    )
    .await
    {
        Ok(Ok(text)) => {
            tracing::info!("Local STT: transcription complete");
            Ok(text)
        }
        Ok(Err(e)) => {
            tracing::error!("Local STT: transcription failed: {}", e);
            Err(e)
        }
        Err(_) => {
            tracing::error!("Local STT: transcription timed out after 300s");
            anyhow::bail!("Local STT transcription timed out (300s)")
        }
    }
}
