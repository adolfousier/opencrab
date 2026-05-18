//! Generate Image Tool
//!
//! Generates images from text prompts. Two wire backends:
//!
//! * **Gemini** — historical default. Calls
//!   `POST https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent`
//!   with `x-goog-api-key` and `responseModalities: ["TEXT", "IMAGE"]`.
//! * **OpenAI-compatible** — `POST {base_url}/images/generations` with
//!   `Authorization: Bearer {key}` and `response_format: "b64_json"`.
//!   Lets users point `generate_image` at OpenRouter, OpenAI, Together,
//!   or any custom provider that exposes the OpenAI images endpoint by
//!   setting `[providers.<name>] generation_model = "..."` in
//!   `config.toml` (and api_key + base_url already there for chat).
//!
//! Both backends save the result as a PNG file in
//! `~/.opencrabs/images/` and return the path.

use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;

const GEMINI_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";
/// Substring match on the active provider's `base_url` is enough to
/// decide which wire protocol to use — Google's images endpoint lives
/// under this host, everyone else's `/v1/images/generations` follows the
/// OpenAI shape.
pub const GEMINI_HOST_MARKER: &str = "generativelanguage.googleapis.com";

/// Which HTTP shape to use for the actual call.
#[derive(Debug, Clone)]
enum Backend {
    Gemini { api_key: String },
    OpenAi { api_key: String, base_url: String },
}

/// Image generation tool — Google Gemini or any OpenAI-compatible
/// `/v1/images/generations` endpoint, picked at construction.
pub struct GenerateImageTool {
    backend: Backend,
    model: String,
}

impl GenerateImageTool {
    /// Historical constructor — Gemini backend, model defaults to
    /// whatever `cli/ui.rs` resolved from
    /// `effective_generation_model(config)`.
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            backend: Backend::Gemini { api_key },
            model,
        }
    }

    /// OpenAI-compatible backend — `base_url` should be the API root
    /// without a trailing slash or `/chat/completions` suffix
    /// (`active_provider_generation` already normalises that).
    pub fn with_openai_backend(api_key: String, base_url: String, model: String) -> Self {
        Self {
            backend: Backend::OpenAi { api_key, base_url },
            model,
        }
    }

    /// Resolve provider config → concrete tool. Returns `None` when image
    /// generation isn't enabled or no key is configured. Picks the
    /// backend by `base_url` shape so a Gemini-provider override (e.g.
    /// `imagen-4.0-generate-001`) still routes through the Gemini API
    /// rather than misfiring at an OpenAI endpoint that doesn't exist
    /// there.
    pub fn from_config(config: &crate::config::Config) -> Option<Self> {
        if !config.image.generation.enabled {
            return None;
        }
        // Per-provider override wins. Backend chosen by URL shape so
        // `generation_model = "imagen-4.0-generate-001"` on the Gemini
        // provider stays on the Gemini wire, while the same field on an
        // OpenRouter / OpenAI / custom provider takes the
        // `/v1/images/generations` path.
        if let Some((api_key, base_url, model)) =
            crate::brain::provider::factory::active_provider_generation(config)
        {
            return Some(if base_url.contains(GEMINI_HOST_MARKER) {
                Self::new(api_key, model)
            } else {
                Self::with_openai_backend(api_key, base_url, model)
            });
        }
        // No provider override — fall back to the global image.generation
        // config (Gemini-only, since the seeded path is Google's).
        let api_key = config.image.generation.api_key.as_ref()?.clone();
        Some(Self::new(api_key, config.image.generation.model.clone()))
    }
}

#[async_trait]
impl Tool for GenerateImageTool {
    fn name(&self) -> &str {
        "generate_image"
    }

    fn description(&self) -> &str {
        "Generate an image from a text prompt. Returns the file path to the saved PNG. Use <<IMG:path>> syntax in your reply to send the image through a channel."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Text description of the image to generate"
                },
                "filename": {
                    "type": "string",
                    "description": "Optional filename (without path). Defaults to a UUID-based name."
                }
            },
            "required": ["prompt"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::Network, ToolCapability::WriteFiles]
    }

    fn requires_approval(&self) -> bool {
        false
    }

    async fn execute(
        &self,
        input: Value,
        _context: &ToolExecutionContext,
    ) -> super::error::Result<ToolResult> {
        let prompt = match input["prompt"].as_str() {
            Some(p) if !p.is_empty() => p.to_string(),
            _ => {
                return Ok(ToolResult::error(
                    "Missing required parameter: prompt".to_string(),
                ));
            }
        };

        let filename = input["filename"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("{}.png", uuid::Uuid::new_v4().simple()));

        // Ensure images directory exists
        let images_dir = crate::config::opencrabs_home().join("images");
        if let Err(e) = tokio::fs::create_dir_all(&images_dir).await {
            return Ok(ToolResult::error(format!(
                "Failed to create images directory: {}",
                e
            )));
        }
        let save_path = images_dir.join(&filename);

        match &self.backend {
            Backend::Gemini { api_key } => self.execute_gemini(&prompt, api_key, &save_path).await,
            Backend::OpenAi { api_key, base_url } => {
                self.execute_openai(&prompt, api_key, base_url, &save_path)
                    .await
            }
        }
    }
}

impl GenerateImageTool {
    async fn execute_gemini(
        &self,
        prompt: &str,
        api_key: &str,
        save_path: &std::path::Path,
    ) -> super::error::Result<ToolResult> {
        let url = format!("{}/models/{}:generateContent", GEMINI_BASE_URL, self.model);
        let body = serde_json::json!({
            "contents": [{"parts": [{"text": prompt}]}],
            "generationConfig": {
                "responseModalities": ["TEXT", "IMAGE"]
            }
        });

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| super::error::ToolError::Execution(e.to_string()))?;

        let response = client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("x-goog-api-key", api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| super::error::ToolError::Execution(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let err_body = response.text().await.unwrap_or_default();
            return Ok(ToolResult::error(format!(
                "Gemini API error {}: {}",
                status, err_body
            )));
        }

        let json: Value = response
            .json()
            .await
            .map_err(|e| super::error::ToolError::Execution(e.to_string()))?;

        let empty_vec = vec![];
        let candidates = json["candidates"].as_array().unwrap_or(&empty_vec);
        let mut image_data: Option<String> = None;
        let mut text_response = String::new();

        'outer: for candidate in candidates {
            let empty_parts = vec![];
            let parts = candidate["content"]["parts"]
                .as_array()
                .unwrap_or(&empty_parts);
            for part in parts {
                if let Some(text) = part["text"].as_str() {
                    text_response.push_str(text);
                }
                if let Some(data) = part["inlineData"]["data"].as_str() {
                    image_data = Some(data.to_string());
                    break 'outer;
                }
            }
        }

        match image_data {
            Some(b64) => save_decoded_image(&b64, save_path, &text_response).await,
            None => {
                if !text_response.is_empty() {
                    Ok(ToolResult::success(format!(
                        "No image generated. Gemini response: {}",
                        text_response
                    )))
                } else {
                    Ok(ToolResult::error(
                        "No image data found in Gemini response".to_string(),
                    ))
                }
            }
        }
    }

    async fn execute_openai(
        &self,
        prompt: &str,
        api_key: &str,
        base_url: &str,
        save_path: &std::path::Path,
    ) -> super::error::Result<ToolResult> {
        // OpenAI `/v1/images/generations` shape — matches OpenAI,
        // OpenRouter, Together, and most clones. `response_format =
        // b64_json` keeps the byte path local; providers that only
        // emit URLs fall through into the URL-fetch branch below.
        let url = format!("{}/images/generations", base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "model": self.model,
            "prompt": prompt,
            "n": 1,
            "response_format": "b64_json",
        });

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| super::error::ToolError::Execution(e.to_string()))?;

        let response = client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| super::error::ToolError::Execution(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let err_body = response.text().await.unwrap_or_default();
            return Ok(ToolResult::error(format!(
                "OpenAI images API error {}: {}",
                status, err_body
            )));
        }

        let json: Value = response
            .json()
            .await
            .map_err(|e| super::error::ToolError::Execution(e.to_string()))?;

        let first = json["data"]
            .as_array()
            .and_then(|a| a.first())
            .cloned()
            .unwrap_or(Value::Null);

        if let Some(b64) = first["b64_json"].as_str() {
            return save_decoded_image(b64, save_path, "").await;
        }

        if let Some(url) = first["url"].as_str() {
            let bytes = client
                .get(url)
                .send()
                .await
                .map_err(|e| super::error::ToolError::Execution(e.to_string()))?
                .bytes()
                .await
                .map_err(|e| super::error::ToolError::Execution(e.to_string()))?;
            tokio::fs::write(save_path, &bytes)
                .await
                .map_err(|e| super::error::ToolError::Execution(e.to_string()))?;
            let path_str = save_path.to_string_lossy().to_string();
            return Ok(ToolResult::success(format!(
                "Generated image saved to: {}\nUse <<IMG:{}>> to reference it.",
                path_str, path_str
            )));
        }

        Ok(ToolResult::error(format!(
            "No image data found in OpenAI-images response: {}",
            json
        )))
    }
}

async fn save_decoded_image(
    b64: &str,
    save_path: &std::path::Path,
    leading_text: &str,
) -> super::error::Result<ToolResult> {
    let bytes = match base64_decode(b64) {
        Ok(b) => b,
        Err(e) => {
            return Ok(ToolResult::error(format!(
                "Failed to decode image data: {}",
                e
            )));
        }
    };
    tokio::fs::write(save_path, &bytes)
        .await
        .map_err(|e| super::error::ToolError::Execution(e.to_string()))?;
    let path_str = save_path.to_string_lossy().to_string();
    let mut output = format!(
        "Generated image saved to: {}\nUse <<IMG:{}>> to reference it.",
        path_str, path_str
    );
    if !leading_text.trim().is_empty() {
        output = format!("{}\n\n{}", leading_text.trim(), output);
    }
    Ok(ToolResult::success(output))
}

fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    // Use base64 via the standard approach — decode without padding issues
    let clean: String = input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '+' || *c == '/' || *c == '=')
        .collect();
    base64_decode_inner(&clean)
}

fn base64_decode_inner(input: &str) -> Result<Vec<u8>, String> {
    // Simple base64 decode without external crate (reqwest already depends on base64 indirectly)
    // Use the engine from the existing base64 crate that reqwest pulls in
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(input)
        .map_err(|e| e.to_string())
}
