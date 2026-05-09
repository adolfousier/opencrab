//! Analyze Video Tool — Gemini-native video understanding.
//!
//! Phase 1: Gemini-only. When the user attaches a video to a channel/TUI
//! session, the file_extract pipeline emits `<<VID:path>>` and the agent
//! invokes this tool. Two upload paths depending on file size:
//!
//! * **Inline (≤ ~18 MB)**: base64 the bytes and pass them directly inside
//!   `inline_data` on `generateContent`. One round-trip, simplest path.
//! * **Files API (> 18 MB)**: resumable upload to `/upload/v1beta/files`,
//!   poll the file resource until `state == "ACTIVE"`, then reference it
//!   via `file_data: { file_uri }` in `generateContent`.
//!
//! Phase 2 (separate tool wiring) will add a frame-extraction fallback so
//! sessions on non-Gemini providers still get something useful.

use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::time::Duration;

const GEMINI_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";
const GEMINI_UPLOAD_URL: &str = "https://generativelanguage.googleapis.com/upload/v1beta/files";

/// Above this byte threshold we route to the Files API instead of inlining
/// base64 into `generateContent`. 18 MB leaves headroom under Gemini's
/// documented 20 MB inline cap once base64 + JSON wrapping is accounted for
/// (4/3 expansion + escaping + envelope ≈ 26 MB on the wire for an 18 MB
/// payload, still safely below request-size limits in practice).
const INLINE_MAX_BYTES: u64 = 18 * 1024 * 1024;

/// How long to poll a Files-API upload waiting for `state: ACTIVE`.
const FILES_API_POLL_TIMEOUT: Duration = Duration::from_secs(120);
/// Interval between Files-API state checks while polling.
const FILES_API_POLL_INTERVAL: Duration = Duration::from_secs(2);

pub struct AnalyzeVideoTool {
    api_key: String,
    model: String,
}

impl AnalyzeVideoTool {
    pub fn new(api_key: String, model: String) -> Self {
        Self { api_key, model }
    }
}

#[async_trait]
impl Tool for AnalyzeVideoTool {
    fn name(&self) -> &str {
        "analyze_video"
    }

    fn description(&self) -> &str {
        "Analyze a video file (local path) using Google Gemini multimodal vision. \
         Use when: the user attached a video and you need to understand its content, \
         the model needs to describe motion / sequence / spoken audio in a video, or \
         a `<<VID:path>>` marker is present in the prompt. Pass `question` to ask \
         something specific (e.g. 'transcribe the spoken audio', 'describe each frame \
         in chronological order'); defaults to a general detailed description. \
         Inline upload for files ≤ 18 MB, otherwise Files API."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "video": {
                    "type": "string",
                    "description": "Local file path to the video (mp4, mov, webm, mkv, avi, 3gp, flv)."
                },
                "question": {
                    "type": "string",
                    "description": "What to ask about the video. Defaults to 'Describe this video in detail — actions, subjects, setting, and any spoken audio in chronological order.'"
                }
            },
            "required": ["video"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::Network, ToolCapability::ReadFiles]
    }

    fn requires_approval(&self) -> bool {
        false
    }

    async fn execute(
        &self,
        input: Value,
        _context: &ToolExecutionContext,
    ) -> super::error::Result<ToolResult> {
        let video_path = match input["video"].as_str() {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => {
                return Ok(ToolResult::error(
                    "Missing required parameter: video".to_string(),
                ));
            }
        };

        let question = input["question"]
            .as_str()
            .unwrap_or(
                "Describe this video in detail — actions, subjects, setting, \
                 and any spoken audio in chronological order.",
            )
            .to_string();

        let metadata = match tokio::fs::metadata(&video_path).await {
            Ok(m) => m,
            Err(e) => {
                return Ok(ToolResult::error(format!(
                    "Failed to stat video file '{}': {}",
                    video_path, e
                )));
            }
        };
        let size = metadata.len();
        let mime_type = detect_video_mime_type(&video_path);

        tracing::info!(
            "analyze_video: path={} size={} mime={} model={}",
            video_path,
            size,
            mime_type,
            self.model,
        );

        let video_part = if size <= INLINE_MAX_BYTES {
            self.build_inline_part(&video_path, mime_type).await?
        } else {
            tracing::info!(
                "analyze_video: file size {} > {} inline cap — using Files API",
                size,
                INLINE_MAX_BYTES,
            );
            self.upload_via_files_api(&video_path, mime_type, size)
                .await?
        };

        // Send generateContent with the video part + question
        self.run_generate_content(video_part, &question).await
    }
}

impl AnalyzeVideoTool {
    /// Read the file, base64 it, and produce an `inline_data` part. Single
    /// round-trip path used for files ≤ INLINE_MAX_BYTES.
    async fn build_inline_part(
        &self,
        path: &str,
        mime_type: &'static str,
    ) -> super::error::Result<Value> {
        let bytes = tokio::fs::read(path).await.map_err(|e| {
            super::error::ToolError::Execution(format!(
                "Failed to read video file '{}': {}",
                path, e
            ))
        })?;
        let b64 = super::analyze_image::base64_encode(&bytes);
        Ok(serde_json::json!({
            "inlineData": {
                "mimeType": mime_type,
                "data": b64
            }
        }))
    }

    /// Resumable upload to the Files API, poll until ACTIVE, return a
    /// `file_data` part referencing the uploaded resource. Used for files
    /// larger than INLINE_MAX_BYTES.
    async fn upload_via_files_api(
        &self,
        path: &str,
        mime_type: &'static str,
        size: u64,
    ) -> super::error::Result<Value> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(600))
            .build()
            .map_err(|e| super::error::ToolError::Execution(e.to_string()))?;

        // Step 1: start a resumable upload session — Gemini returns the
        // upload URL in the X-Goog-Upload-URL response header.
        let display_name = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("video");
        let init_body = serde_json::json!({
            "file": { "display_name": display_name }
        });
        let init_resp = client
            .post(GEMINI_UPLOAD_URL)
            .header("x-goog-api-key", &self.api_key)
            .header("X-Goog-Upload-Protocol", "resumable")
            .header("X-Goog-Upload-Command", "start")
            .header("X-Goog-Upload-Header-Content-Length", size.to_string())
            .header("X-Goog-Upload-Header-Content-Type", mime_type)
            .header("Content-Type", "application/json")
            .json(&init_body)
            .send()
            .await
            .map_err(|e| super::error::ToolError::Execution(e.to_string()))?;

        if !init_resp.status().is_success() {
            let status = init_resp.status();
            let body = init_resp.text().await.unwrap_or_default();
            return Err(super::error::ToolError::Execution(format!(
                "Files API resumable-start failed: HTTP {} — {}",
                status, body
            )));
        }
        let upload_url = init_resp
            .headers()
            .get("x-goog-upload-url")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .ok_or_else(|| {
                super::error::ToolError::Execution(
                    "Files API resumable-start: missing X-Goog-Upload-URL header".to_string(),
                )
            })?;

        // Step 2: PUT the bytes (one shot, finalize in same call).
        let bytes = tokio::fs::read(path).await.map_err(|e| {
            super::error::ToolError::Execution(format!(
                "Failed to read video file '{}': {}",
                path, e
            ))
        })?;
        let upload_resp = client
            .post(&upload_url)
            .header("Content-Length", bytes.len().to_string())
            .header("X-Goog-Upload-Offset", "0")
            .header("X-Goog-Upload-Command", "upload, finalize")
            .body(bytes)
            .send()
            .await
            .map_err(|e| super::error::ToolError::Execution(e.to_string()))?;

        if !upload_resp.status().is_success() {
            let status = upload_resp.status();
            let body = upload_resp.text().await.unwrap_or_default();
            return Err(super::error::ToolError::Execution(format!(
                "Files API upload failed: HTTP {} — {}",
                status, body
            )));
        }
        let upload_json: Value = upload_resp.json().await.map_err(|e| {
            super::error::ToolError::Execution(format!(
                "Files API upload: failed to parse JSON response: {}",
                e
            ))
        })?;
        let file_name = upload_json["file"]["name"]
            .as_str()
            .ok_or_else(|| {
                super::error::ToolError::Execution(
                    "Files API upload: missing file.name in response".to_string(),
                )
            })?
            .to_string();
        let file_uri = upload_json["file"]["uri"]
            .as_str()
            .ok_or_else(|| {
                super::error::ToolError::Execution(
                    "Files API upload: missing file.uri in response".to_string(),
                )
            })?
            .to_string();

        // Step 3: poll until state == "ACTIVE" (or timeout). Video files
        // need server-side processing before they can be referenced in
        // generateContent.
        let deadline = std::time::Instant::now() + FILES_API_POLL_TIMEOUT;
        loop {
            if std::time::Instant::now() >= deadline {
                return Err(super::error::ToolError::Execution(format!(
                    "Files API upload: file '{}' did not reach ACTIVE state within {}s",
                    file_name,
                    FILES_API_POLL_TIMEOUT.as_secs()
                )));
            }
            let status_resp = client
                .get(format!("{}/{}", GEMINI_BASE_URL, file_name))
                .header("x-goog-api-key", &self.api_key)
                .send()
                .await
                .map_err(|e| super::error::ToolError::Execution(e.to_string()))?;
            if !status_resp.status().is_success() {
                let status = status_resp.status();
                let body = status_resp.text().await.unwrap_or_default();
                return Err(super::error::ToolError::Execution(format!(
                    "Files API state poll failed: HTTP {} — {}",
                    status, body
                )));
            }
            let status_json: Value = status_resp.json().await.map_err(|e| {
                super::error::ToolError::Execution(format!(
                    "Files API state poll: failed to parse JSON: {}",
                    e
                ))
            })?;
            let state = status_json["state"].as_str().unwrap_or("").to_string();
            tracing::debug!("analyze_video: file '{}' state={}", file_name, state);
            match state.as_str() {
                "ACTIVE" => break,
                "FAILED" => {
                    return Err(super::error::ToolError::Execution(format!(
                        "Files API: upload '{}' entered FAILED state",
                        file_name
                    )));
                }
                _ => {
                    tokio::time::sleep(FILES_API_POLL_INTERVAL).await;
                }
            }
        }

        Ok(serde_json::json!({
            "fileData": {
                "mimeType": mime_type,
                "fileUri": file_uri
            }
        }))
    }

    /// POST a `generateContent` request with the video part + the user's
    /// question, parse out the assembled text response.
    async fn run_generate_content(
        &self,
        video_part: Value,
        question: &str,
    ) -> super::error::Result<ToolResult> {
        let url = format!("{}/models/{}:generateContent", GEMINI_BASE_URL, self.model);
        let body = serde_json::json!({
            "contents": [{
                "parts": [
                    video_part,
                    { "text": question }
                ]
            }]
        });

        // Generous timeout — video processing can take a while server-side
        // even after Files-API ACTIVE.
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(|e| super::error::ToolError::Execution(e.to_string()))?;

        tracing::info!(
            "analyze_video: calling Gemini generateContent model={} url={}",
            self.model,
            url,
        );

        let response = client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("x-goog-api-key", &self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| super::error::ToolError::Execution(e.to_string()))?;

        let status = response.status().as_u16();
        let body_text = response.text().await.map_err(|e| {
            super::error::ToolError::Execution(format!("Failed to read response body: {}", e))
        })?;

        tracing::info!(
            "analyze_video: Gemini HTTP status={} body[..300]={}",
            status,
            &body_text.chars().take(300).collect::<String>()
        );

        if !(200..300).contains(&status) {
            return Ok(ToolResult::error(format!(
                "Gemini API error {}: {}",
                status, body_text
            )));
        }

        let json: Value = serde_json::from_str(&body_text).map_err(|e| {
            super::error::ToolError::Execution(format!(
                "Failed to parse Gemini JSON response: {}. Body[..500]: {}",
                e,
                &body_text.chars().take(500).collect::<String>()
            ))
        })?;

        let empty_vec = vec![];
        let candidates = json["candidates"].as_array().unwrap_or(&empty_vec);
        let mut result_text = String::new();
        for candidate in candidates {
            let empty_parts = vec![];
            let parts = candidate["content"]["parts"]
                .as_array()
                .unwrap_or(&empty_parts);
            for part in parts {
                if let Some(text) = part["text"].as_str() {
                    result_text.push_str(text);
                }
            }
        }

        if result_text.is_empty() {
            Ok(ToolResult::error(
                "No text response from Gemini video analysis".to_string(),
            ))
        } else {
            Ok(ToolResult::success(result_text))
        }
    }
}

fn detect_video_mime_type(path: &str) -> &'static str {
    let lower = path.to_lowercase();
    if lower.ends_with(".mp4") || lower.ends_with(".m4v") {
        "video/mp4"
    } else if lower.ends_with(".mov") {
        "video/quicktime"
    } else if lower.ends_with(".webm") {
        "video/webm"
    } else if lower.ends_with(".mkv") {
        "video/x-matroska"
    } else if lower.ends_with(".avi") {
        "video/x-msvideo"
    } else if lower.ends_with(".3gp") {
        "video/3gpp"
    } else if lower.ends_with(".flv") {
        "video/x-flv"
    } else {
        "video/mp4"
    }
}
