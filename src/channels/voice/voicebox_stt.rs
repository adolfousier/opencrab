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
        anyhow::bail!(
            "Voicebox STT error ({}): {}",
            status,
            translate_voicebox_error(&error_text),
        );
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

/// Translate known voicebox server error patterns into a short, actionable
/// message instead of dumping the raw Python traceback / PyInstaller
/// internals to the end user. The raw text is returned unchanged when no
/// pattern matches, so unknown failures still surface their original
/// detail for debugging.
///
/// Patterns currently handled:
/// - `Cannot load imports from non-existent stub '.../librosa/...'`
///   (and any other `.../<package>/__init__.pyi`) — voicebox was shipped
///   via PyInstaller without `.pyi` stub files that `lazy_loader` needs
///   at runtime. Rebuild the voicebox bundle with
///   `--collect-data <package>` and a `hooks/hook-lazy_loader.py` that
///   sets `hiddenimports = ['lazy_loader']`. The librosa case bit us
///   on 2026-05-23.
pub(crate) fn translate_voicebox_error(raw: &str) -> String {
    if raw.contains("Cannot load imports from non-existent stub") && raw.contains("__init__.pyi") {
        let pkg = extract_package_from_stub_error(raw).unwrap_or_else(|| "<unknown>".to_string());
        return format!(
            "Voicebox is missing the `{pkg}` runtime stubs (likely a PyInstaller \
             bundle issue — lazy_loader needs the `__init__.pyi` files at runtime \
             but the build stripped them). Rebuild voicebox with \
             `--collect-data {pkg}` and a `hooks/hook-lazy_loader.py` that sets \
             `hiddenimports = ['lazy_loader']`. Original detail: {raw}"
        );
    }
    raw.to_string()
}

/// Pull the failing package name out of an error string like
/// `Cannot load imports from non-existent stub '/.../librosa/core/__init__.pyi'`.
/// Returns just the top-level package (e.g. `librosa`), which is the
/// argument the user needs to pass to `--collect-data`.
fn extract_package_from_stub_error(raw: &str) -> Option<String> {
    let after_marker = raw.split("__init__.pyi").next()?;
    // `after_marker` ends right before `__init__.pyi`. The path before
    // it sits between the opening `'` (or `"`) and the end-of-string,
    // so taking the LAST segment after splitting on the quote yields
    // the path itself.
    let path_part = after_marker.rsplit(['\'', '"']).next()?;
    let mei_split = path_part.split("_MEI").nth(1)?;
    // `mei_split` looks like "xyz123/librosa/core/" — split on / and
    // skip the first segment (the random suffix).
    let mut segments = mei_split.split('/').filter(|s| !s.is_empty());
    let _suffix = segments.next();
    segments.next().map(|s| s.to_string())
}
