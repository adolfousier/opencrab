//! Tests for voice/service.rs: STT transcription and TTS synthesis
//! with mock HTTP servers.

use crate::channels::voice::service::{synthesize_speech_with_url, transcribe_audio_with_url};

// ── TranscriptionResponse ─────────────────────────────────────────────

#[test]
fn transcription_response_parse() {
    use crate::channels::voice::service::TranscriptionResponse;
    let json = r#"{"text": "Hello, this is a test."}"#;
    let result: TranscriptionResponse = serde_json::from_str(json).unwrap();
    assert_eq!(result.text, "Hello, this is a test.");
}

#[test]
fn transcription_response_parse_unicode() {
    use crate::channels::voice::service::TranscriptionResponse;
    let json = r#"{"text": "Olá, como você está?"}"#;
    let result: TranscriptionResponse = serde_json::from_str(json).unwrap();
    assert_eq!(result.text, "Olá, como você está?");
}

#[test]
fn transcription_response_parse_empty() {
    use crate::channels::voice::service::TranscriptionResponse;
    let json = r#"{"text": ""}"#;
    let result: TranscriptionResponse = serde_json::from_str(json).unwrap();
    assert_eq!(result.text, "");
}

// ── STT tests ─────────────────────────────────────────────────────────

#[tokio::test]
async fn stt_success() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/")
        .match_header("Authorization", "Bearer test-groq-key")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"text": "Hello from voice note"}"#)
        .create_async()
        .await;

    let audio = vec![0u8; 100];
    let result = transcribe_audio_with_url(audio, "test-groq-key", &server.url()).await;

    mock.assert_async().await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "Hello from voice note");
}

#[tokio::test]
async fn stt_api_error_returns_error() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/")
        .with_status(401)
        .with_body(r#"{"error": "Invalid API key"}"#)
        .create_async()
        .await;

    let audio = vec![0u8; 50];
    let result = transcribe_audio_with_url(audio, "bad-key", &server.url()).await;

    mock.assert_async().await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("401"));
}

#[tokio::test]
async fn stt_server_error_500() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/")
        .with_status(500)
        .with_body("Internal Server Error")
        .create_async()
        .await;

    let audio = vec![0u8; 50];
    let result = transcribe_audio_with_url(audio, "key", &server.url()).await;

    mock.assert_async().await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("500"));
}

#[tokio::test]
async fn stt_malformed_json_response() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("not json at all")
        .create_async()
        .await;

    let audio = vec![0u8; 50];
    let result = transcribe_audio_with_url(audio, "key", &server.url()).await;

    mock.assert_async().await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("parse"));
}

#[tokio::test]
async fn stt_long_transcription() {
    let long_text = "word ".repeat(500);
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(r#"{{"text": "{}"}}"#, long_text.trim()))
        .create_async()
        .await;

    let audio = vec![0u8; 100];
    let result = transcribe_audio_with_url(audio, "key", &server.url()).await;

    mock.assert_async().await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), long_text.trim());
}

// ── TTS tests ─────────────────────────────────────────────────────────

#[tokio::test]
async fn tts_success() {
    let fake_audio = vec![0xFFu8; 256];
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/")
        .match_header("Authorization", "Bearer test-openai-key")
        .with_status(200)
        .with_header("content-type", "audio/opus")
        .with_body(fake_audio.clone())
        .create_async()
        .await;

    let result = synthesize_speech_with_url(
        "Hello world",
        "test-openai-key",
        "ash",
        "gpt-4o-mini-tts",
        &server.url(),
    )
    .await;

    mock.assert_async().await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), fake_audio);
}

#[tokio::test]
async fn tts_sends_correct_json_body() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/")
        .match_header("content-type", "application/json")
        .match_body(mockito::Matcher::PartialJsonString(
            r#"{"model":"gpt-4o-mini-tts","voice":"ash","response_format":"opus"}"#.to_string(),
        ))
        .with_status(200)
        .with_body(vec![0u8; 10])
        .create_async()
        .await;

    let _ =
        synthesize_speech_with_url("Test input", "key", "ash", "gpt-4o-mini-tts", &server.url())
            .await;

    mock.assert_async().await;
}

#[tokio::test]
async fn tts_api_error_returns_error() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/")
        .with_status(429)
        .with_body(r#"{"error": "Rate limit exceeded"}"#)
        .create_async()
        .await;

    let result =
        synthesize_speech_with_url("Hello", "key", "ash", "gpt-4o-mini-tts", &server.url()).await;

    mock.assert_async().await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("429"));
}

#[tokio::test]
async fn tts_server_error_500() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/")
        .with_status(500)
        .with_body("Internal Server Error")
        .create_async()
        .await;

    let result = synthesize_speech_with_url("Hello", "key", "ash", "tts-1", &server.url()).await;

    mock.assert_async().await;
    assert!(result.is_err());
}

#[tokio::test]
async fn tts_empty_audio_response() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/")
        .with_status(200)
        .with_body(Vec::<u8>::new())
        .create_async()
        .await;

    let result = synthesize_speech_with_url("Hello", "key", "ash", "tts-1", &server.url()).await;

    mock.assert_async().await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn tts_different_voices() {
    for voice in &["ash", "alloy", "nova", "shimmer"] {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/")
            .match_body(mockito::Matcher::PartialJsonString(format!(
                r#"{{"voice":"{}"}}"#,
                voice
            )))
            .with_status(200)
            .with_body(vec![1u8; 10])
            .create_async()
            .await;

        let result = synthesize_speech_with_url("Test", "key", voice, "tts-1", &server.url()).await;

        mock.assert_async().await;
        assert!(result.is_ok(), "voice '{}' should work", voice);
    }
}
