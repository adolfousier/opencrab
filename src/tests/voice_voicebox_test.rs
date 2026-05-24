//! Tests for Voicebox TTS and STT providers.
//!
//! Uses mockito to simulate `/generate` and `/transcribe` endpoints.

#[cfg(test)]
mod tests {
    use crate::channels::voice::voicebox_stt;
    use crate::channels::voice::voicebox_tts::VoiceboxTts;

    // ─── TTS ────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn tts_new_creates_client() {
        let _tts = VoiceboxTts::new("http://localhost:8000", "profile-abc", "xtts");
        // Just verify it doesn't panic and stores the values
        // (fields are private, so we test via synthesize behavior)
    }

    #[tokio::test]
    async fn tts_empty_text_rejected() {
        let tts = VoiceboxTts::new("http://localhost:8000", "profile-abc", "xtts");
        let result = tts.synthesize("").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty text"));
    }

    #[tokio::test]
    async fn tts_successful_synthesis_reads_audio_file() {
        let mut server = mockito::Server::new_async().await;
        let mock_url = server.url();

        let fake_audio = vec![0x52, 0x49, 0x46, 0x46, 0x00, 0x01, 0x02, 0x03];

        // Mock POST /generate → returns completed with audio path
        let _generate = server
            .mock("POST", "/generate")
            .match_body(mockito::Matcher::Json(serde_json::json!({
                "profile_id": "profile-abc",
                "text": "hello voicebox",
                "engine": "xtts"
            })))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "id": "gen-123",
                    "status": "completed",
                    "audio_path": "/audio/gen-123.wav",
                    "duration": 1.5,
                    "error": null
                })
                .to_string(),
            )
            .create_async()
            .await;

        // Mock GET /audio/gen-123.wav → returns audio bytes
        let _audio = server
            .mock("GET", "/audio/gen-123.wav")
            .with_status(200)
            .with_body(fake_audio.clone())
            .create_async()
            .await;

        let tts = VoiceboxTts::new(&mock_url, "profile-abc", "xtts");
        let result = tts.synthesize("hello voicebox").await;

        assert!(result.is_ok());
        let bytes = result.unwrap();
        assert_eq!(bytes, fake_audio);
    }

    #[tokio::test]
    async fn tts_audio_fetch_fails_on_404() {
        let mut server = mockito::Server::new_async().await;
        let mock_url = server.url();

        let _generate = server
            .mock("POST", "/generate")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "id": "gen-123",
                    "status": "completed",
                    "audio_path": "/audio/missing.wav",
                    "duration": 1.0,
                    "error": null
                })
                .to_string(),
            )
            .create_async()
            .await;

        // Audio endpoint returns 404
        let _audio = server
            .mock("GET", "/audio/missing.wav")
            .with_status(404)
            .with_body("not found")
            .create_async()
            .await;

        let tts = VoiceboxTts::new(&mock_url, "profile-abc", "xtts");
        let result = tts.synthesize("hello").await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("404") || err.contains("error"));
    }

    #[tokio::test]
    async fn tts_server_error_propagated() {
        let mut server = mockito::Server::new_async().await;
        let mock_url = server.url();

        let _mock = server
            .mock("POST", "/generate")
            .with_status(500)
            .with_body(r#"{"error": "GPU out of memory"}"#)
            .create_async()
            .await;

        let tts = VoiceboxTts::new(&mock_url, "profile-abc", "xtts");
        let result = tts.synthesize("hello").await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("500"));
        assert!(err.contains("GPU out of memory"));
    }

    #[tokio::test]
    async fn tts_connection_refused_on_bad_url() {
        let tts = VoiceboxTts::new("http://localhost:1", "profile", "xtts");
        let result = tts.synthesize("hello").await;
        assert!(result.is_err());
    }

    // ─── STT ────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn stt_mocks_successful_transcription() {
        let mut server = mockito::Server::new_async().await;
        let mock_url = server.url();

        let _mock = server
            .mock("POST", "/transcribe")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"text": "voicebox transcription works"}"#)
            .create_async()
            .await;

        let result = voicebox_stt::transcribe(vec![0x00, 0x01, 0x02, 0x03], &mock_url).await;

        assert!(result.is_ok());
        let text = result.unwrap();
        assert_eq!(text, "voicebox transcription works");
    }

    #[tokio::test]
    async fn stt_server_error_propagated() {
        let mut server = mockito::Server::new_async().await;
        let mock_url = server.url();

        let _mock = server
            .mock("POST", "/transcribe")
            .with_status(503)
            .with_body(r#"{"error": "STT model not loaded"}"#)
            .create_async()
            .await;

        let result = voicebox_stt::transcribe(vec![0x00, 0x01], &mock_url).await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("503"));
        assert!(err.contains("STT model not loaded"));
    }

    #[tokio::test]
    async fn stt_invalid_json_response() {
        let mut server = mockito::Server::new_async().await;
        let mock_url = server.url();

        // Returns 200 but body is not valid JSON for TranscribeResponse
        let _mock = server
            .mock("POST", "/transcribe")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"not_text": "missing text field"}"#)
            .create_async()
            .await;

        let result = voicebox_stt::transcribe(vec![0x00, 0x01], &mock_url).await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("parse") || err.contains("text") || err.contains("Failed"));
    }

    #[tokio::test]
    async fn stt_connection_refused_on_bad_url() {
        let result = voicebox_stt::transcribe(vec![0x00, 0x01], "http://localhost:1").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn stt_liveness_probe_fails_fast_when_voicebox_down() {
        // Port 1 is reserved and refuses connections immediately. The
        // probe should bail in well under the multipart timeout — we
        // assert <5s as a generous upper bound; in practice it's <100ms.
        let start = std::time::Instant::now();
        let result = voicebox_stt::probe_liveness("http://localhost:1").await;
        let elapsed = start.elapsed();
        assert!(result.is_err(), "probe must fail on connection refused");
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "probe took {elapsed:?} — should fail fast on refused connection",
        );
    }

    #[tokio::test]
    async fn stt_liveness_probe_passes_on_any_http_response() {
        // mockito with no mocks configured for GET / still serves an HTTP
        // response (501). The probe accepts that as "server alive" — the
        // only failure it cares about is the connection itself.
        let server = mockito::Server::new_async().await;
        let url = server.url();
        let result = voicebox_stt::probe_liveness(&url).await;
        assert!(
            result.is_ok(),
            "liveness probe should pass on any HTTP response, got {result:?}",
        );
    }

    #[tokio::test]
    async fn stt_translates_librosa_stub_error_into_actionable_message() {
        // Reproduce the exact 2026-05-23 error body.
        let mut server = mockito::Server::new_async().await;
        let mock_url = server.url();
        let raw_body = r#"{"detail":"Cannot load imports from non-existent stub '/var/folders/w_/p0_1b_hd18n1qgtrt2mwfwv80000gn/T/_MEItwjtji/librosa/core/__init__.pyi'"}"#;
        let _mock = server
            .mock("POST", "/transcribe")
            .with_status(500)
            .with_body(raw_body)
            .create_async()
            .await;

        let result = voicebox_stt::transcribe(vec![0x00, 0x01], &mock_url).await;
        let err = result.unwrap_err().to_string();

        // Top-level package extracted from the path correctly.
        assert!(
            err.contains("librosa"),
            "translation should name the failing package, got: {err}",
        );
        // Actionable rebuild instruction surfaced.
        assert!(
            err.contains("--collect-data librosa"),
            "translation should suggest the rebuild fix, got: {err}",
        );
        // Original detail preserved at the end for full debugging.
        assert!(
            err.contains("Original detail"),
            "translation should keep the raw detail for diagnosis, got: {err}",
        );
    }

    #[tokio::test]
    async fn stt_translator_passes_through_unknown_errors_unchanged() {
        // An error that doesn't match the known librosa/lazy_loader
        // pattern should bubble up verbatim so we don't hide real bugs.
        let mut server = mockito::Server::new_async().await;
        let mock_url = server.url();
        let raw_body = r#"{"error": "completely unexpected new failure mode"}"#;
        let _mock = server
            .mock("POST", "/transcribe")
            .with_status(500)
            .with_body(raw_body)
            .create_async()
            .await;

        let result = voicebox_stt::transcribe(vec![0x00, 0x01], &mock_url).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("completely unexpected new failure mode"));
        // Should NOT have the librosa-pattern messaging.
        assert!(!err.contains("--collect-data"));
    }

    #[tokio::test]
    async fn stt_dead_voicebox_returns_clear_unreachable_message() {
        // The error string must contain "unreachable" so the dispatcher
        // can pattern-match it for fallback decisions, and so the
        // surfaced error to the user is actionable instead of a raw
        // reqwest panic.
        let result = voicebox_stt::transcribe(vec![0x00, 0x01], "http://localhost:1").await;
        let err = result.unwrap_err().to_string();
        assert!(
            err.to_lowercase().contains("unreachable")
                || err.to_lowercase().contains("liveness probe failed"),
            "error should explain voicebox is unreachable, got: {err}",
        );
    }
}
