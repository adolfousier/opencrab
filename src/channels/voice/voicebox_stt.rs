//! Voicebox STT provider.
//!
//! POST to `/transcribe` endpoint with audio file.
//! Returns transcription text.

use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

use super::openai_tts::build_endpoint_url;

/// How long to wait for the liveness probe before declaring voicebox down.
/// 2s is enough for any responsive localhost service and short enough that
/// a dead voicebox doesn't block the STT dispatcher's fallback chain.
const LIVENESS_TIMEOUT: Duration = Duration::from_secs(2);

/// Quick liveness probe so a dead voicebox process fails in ~2s instead of
/// blocking on a 30s+ multipart upload that will time out anyway. Any HTTP
/// response (2xx/3xx/4xx) means the server is reachable and counts as
/// alive; only a connection-level failure (refused, timeout, DNS) returns
/// `Err`. That's the signal the STT dispatcher uses to skip voicebox and
/// try the next provider in the chain.
pub async fn probe_liveness(base_url: &str) -> Result<()> {
    let url = build_endpoint_url(base_url, "")?;
    let client = Client::builder()
        .timeout(LIVENESS_TIMEOUT)
        .build()
        .context("Failed to build liveness probe client")?;
    client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("Voicebox liveness probe failed at {url}"))?;
    Ok(())
}

/// Transcribe audio bytes using Voicebox.
///
/// POST audio file to `/transcribe` endpoint.
/// Returns the transcribed text.
pub async fn transcribe(audio_bytes: Vec<u8>, base_url: &str) -> Result<String> {
    // Liveness probe first so a stopped voicebox doesn't waste 30s on a
    // multipart upload that will time out. The dispatcher catches this
    // failure mode and proceeds to the next provider in the chain.
    if let Err(e) = probe_liveness(base_url).await {
        anyhow::bail!("Voicebox STT unreachable (liveness probe failed): {e}");
    }

    let transcribe_url = build_endpoint_url(base_url, "transcribe")?;

    let client = Client::new();

    let file_part = reqwest::multipart::Part::bytes(audio_bytes)
        .file_name("voice.ogg")
        .mime_str("audio/ogg")?;

    let form = reqwest::multipart::Form::new().part("file", file_part);

    let response = client
        .post(&transcribe_url)
        .multipart(form)
        .send()
        .await
        .context("Failed to send audio to Voicebox STT")?;

    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_default();
        anyhow::bail!("Voicebox STT error ({}): {}", status, error_text);
    }

    let result: TranscribeResponse = response
        .json()
        .await
        .context("Failed to parse Voicebox STT response")?;

    tracing::info!(
        "Voicebox STT: transcribed {} chars (url={})",
        result.text.len(),
        transcribe_url,
    );

    Ok(result.text)
}

#[derive(Deserialize)]
struct TranscribeResponse {
    text: String,
}
