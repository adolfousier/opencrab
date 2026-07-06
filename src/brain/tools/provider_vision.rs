//! Provider Vision Tool
//!
//! Analyzes images by rolling through every provider vision candidate
//! (vision_model + usable key) via the OpenAI-compatible API, then the
//! fallback chain entries, with Gemini `[image.vision]` strictly last
//! (#430). Registered as `analyze_image` whenever any candidate exists.

use super::analyze_image::{AnalyzeImageTool, base64_encode, detect_mime_type};
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;

/// Image vision/analysis tool rolling through provider vision candidates.
pub struct ProviderVisionTool {
    /// Ordered `(api_key, base_url, vision_model)` candidates from
    /// `factory::vision_candidates` (#430). Walked in order at request
    /// time: a candidate failure tries the next, next, next.
    candidates: Vec<(String, String, String)>,
    /// Gemini fallback, tried only after EVERY candidate fails. Gemini is
    /// never primary while any provider can serve vision (owner contract).
    gemini_fallback: Option<AnalyzeImageTool>,
}

impl ProviderVisionTool {
    pub fn new(candidates: Vec<(String, String, String)>) -> Self {
        Self {
            candidates,
            gemini_fallback: None,
        }
    }

    /// Attach a Gemini fallback (`image.vision` key + model). Tried only if the
    /// provider's own vision call fails.
    pub fn with_gemini_fallback(mut self, api_key: String, model: String) -> Self {
        self.gemini_fallback = Some(AnalyzeImageTool::new(api_key, model));
        self
    }

    /// Run the Gemini fallback if present; otherwise return `primary`.
    async fn fallback_or(
        &self,
        input: &Value,
        context: &ToolExecutionContext,
        primary: ToolResult,
        reason: &str,
    ) -> super::error::Result<ToolResult> {
        if let Some(fb) = &self.gemini_fallback {
            tracing::warn!(
                "analyze_image: provider vision failed ({reason}); falling back to Gemini"
            );
            return fb.execute(input.clone(), context).await;
        }
        Ok(primary)
    }
}

#[async_trait]
impl Tool for ProviderVisionTool {
    fn name(&self) -> &str {
        "analyze_image"
    }

    fn description(&self) -> &str {
        "Analyze an image file (local path) or URL using the provider's vision model. \
         Use when: the current model doesn't support vision, you need to analyze a saved file, \
         or the user sends an image. The vision model describes the image so you can understand it."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "image": {
                    "type": "string",
                    "description": "Local file path (e.g. /home/user/photo.png) or HTTPS URL to the image"
                },
                "question": {
                    "type": "string",
                    "description": "What to ask about the image. Defaults to 'Describe this image in detail.'"
                }
            },
            "required": ["image"]
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
        let image_src = match input["image"].as_str() {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => {
                return Ok(ToolResult::error(
                    "Missing required parameter: image".to_string(),
                ));
            }
        };

        let question = input["question"]
            .as_str()
            .unwrap_or("Describe this image in detail.")
            .to_string();

        // Build image_url content part
        let image_url = if image_src.starts_with("http://") || image_src.starts_with("https://") {
            image_src.clone()
        } else {
            // Local file — read and base64 encode
            let bytes = tokio::fs::read(&image_src).await.map_err(|e| {
                super::error::ToolError::Execution(format!(
                    "Failed to read image file '{}': {}",
                    image_src, e
                ))
            })?;
            let mime = detect_mime_type(&image_src);
            let b64 = base64_encode(&bytes);
            format!("data:{};base64,{}", mime, b64)
        };

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| super::error::ToolError::Execution(e.to_string()))?;

        // Roll through candidates in order (#430): a failure logs and tries
        // the next; Gemini runs only after every candidate failed.
        let mut last_failure = "no vision candidates configured".to_string();
        for (api_key, base_url, vision_model) in &self.candidates {
            match try_vision_candidate(
                &client,
                api_key,
                base_url,
                vision_model,
                &question,
                &image_url,
            )
            .await
            {
                Ok(text) => return Ok(ToolResult::success(text)),
                Err(reason) => {
                    tracing::warn!(
                        "analyze_image: vision candidate model={vision_model} url={base_url} \
                         failed: {reason} — trying next candidate"
                    );
                    last_failure = reason;
                }
            }
        }
        self.fallback_or(
            &input,
            _context,
            ToolResult::error(format!(
                "All provider vision candidates failed: {last_failure}"
            )),
            &last_failure,
        )
        .await
    }
}

/// One OpenAI-compatible vision call. `Err` carries the reason so the
/// caller can log it and roll to the next candidate.
async fn try_vision_candidate(
    client: &reqwest::Client,
    api_key: &str,
    base_url: &str,
    vision_model: &str,
    question: &str,
    image_url: &str,
) -> std::result::Result<String, String> {
    let body = serde_json::json!({
        "model": vision_model,
        "messages": [{
            "role": "user",
            "content": [
                { "type": "text", "text": question },
                { "type": "image_url", "image_url": { "url": image_url } }
            ]
        }],
        "max_tokens": 1024
    });

    let response = client
        .post(base_url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Vision request failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let err_body = response.text().await.unwrap_or_default();
        return Err(format!("Vision API error {status}: {err_body}"));
    }

    let json: Value = response
        .json()
        .await
        .map_err(|e| format!("Vision response was not JSON: {e}"))?;
    let result_text = json["choices"]
        .as_array()
        .and_then(|choices| choices.first())
        .and_then(|choice| choice["message"]["content"].as_str())
        .unwrap_or("")
        .to_string();
    if result_text.is_empty() {
        return Err("No text response from vision model".to_string());
    }
    Ok(result_text)
}

/// Placeholder `analyze_image` registered when no vision backend is configured
/// (no provider `vision_model`, no Gemini `image.vision` key). Instead of the
/// tool simply being absent — which leaves the agent unable to explain why it
/// can't see an image — this returns a clear, actionable setup hint the agent
/// relays to the user on the TUI or a channel.
pub struct VisionSetupHintTool;

#[async_trait]
impl Tool for VisionSetupHintTool {
    fn name(&self) -> &str {
        "analyze_image"
    }

    fn description(&self) -> &str {
        "Analyze an image. NOTE: image analysis is not configured on this \
         install yet — calling this returns setup instructions to relay to the \
         user."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "image": { "type": "string", "description": "Image path or URL" }
            },
            "required": ["image"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![]
    }

    fn requires_approval(&self) -> bool {
        false
    }

    async fn execute(
        &self,
        _input: Value,
        _context: &ToolExecutionContext,
    ) -> super::error::Result<ToolResult> {
        Ok(ToolResult::error(
            "Image analysis isn't set up yet. To enable it, either: (1) set a \
             multimodal `vision_model` on your active provider via the \
             `config_manager` tool (works for OpenAI-compatible providers), \
             or (2) add a Google Gemini vision key via the `/onboard:image` \
             wizard (or an `[image.vision]` section). It hot-reloads, so no \
             restart is needed. Tell the user this."
                .to_string(),
        ))
    }
}
