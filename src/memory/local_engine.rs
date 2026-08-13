//! Local GGUF embedding engine — llama-cpp-2, owned by OpenCrabs.
//!
//! This replaced `qmd::llm` when the qmd crate was dropped (#1028). It is
//! deliberately the same engine qmd wrapped: same model, same prompt
//! templates, same context-sizing logic, same HuggingFace cache directory.
//! A user upgrading from a qmd-built release keeps their ~300MB model
//! download and gets bit-identical embeddings, so existing vectors in
//! memory.db stay comparable with new ones.
//!
//! What was NOT copied: rerank, generation, query expansion, progress bars
//! (callers already suppress stdio and log via tracing), session management.

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Default embedding model file name.
pub const DEFAULT_EMBED_MODEL: &str = "embeddinggemma-300M-Q8_0.gguf";

/// HuggingFace URI for the default embedding model.
pub const DEFAULT_EMBED_MODEL_URI: &str =
    "hf:ggml-org/embeddinggemma-300M-GGUF/embeddinggemma-300M-Q8_0.gguf";

/// Result of an embedding operation.
#[derive(Debug, Clone)]
pub struct EmbeddingResult {
    /// The embedding vector.
    pub embedding: Vec<f32>,
    /// Model name used.
    pub model: String,
}

/// Result of a model pull.
#[derive(Debug, Clone)]
pub struct PullResult {
    /// The model URI requested.
    pub model: String,
    /// Local path of the model file.
    pub path: PathBuf,
    /// Size on disk in bytes.
    pub size_bytes: u64,
}

/// The embedding engine: a loaded GGUF model behind llama.cpp.
///
/// Not Send-friendly by itself; callers hold it in a Mutex (see
/// `super::embedding`). Each embed call creates a context sized for the
/// input, which is what makes arbitrary-length chunks safe together with
/// the caller-side byte cap.
#[derive(Debug)]
pub struct EmbeddingEngine {
    backend: LlamaBackend,
    model: Arc<LlamaModel>,
    dimensions: Option<usize>,
}

impl EmbeddingEngine {
    /// Load a GGUF model from disk.
    pub fn new(model_path: &Path) -> Result<Self, String> {
        let backend = LlamaBackend::init().map_err(|e| format!("llama backend init: {e}"))?;

        let model_params = LlamaModelParams::default();
        let model = LlamaModel::load_from_file(&backend, model_path, &model_params)
            .map_err(|e| format!("Failed to load model {}: {e}", model_path.display()))?;

        Ok(Self {
            backend,
            model: Arc::new(model),
            dimensions: None,
        })
    }

    /// Embed a document chunk, with its title riding along.
    pub fn embed_document(
        &mut self,
        text: &str,
        title: Option<&str>,
    ) -> Result<EmbeddingResult, String> {
        let formatted = format_doc_for_embedding(text, title);
        self.embed_raw(&formatted)
    }

    /// Embed a search query.
    pub fn embed_query(&mut self, query: &str) -> Result<EmbeddingResult, String> {
        let formatted = format_query_for_embedding(query);
        self.embed_raw(&formatted)
    }

    /// Raw embedding generation.
    fn embed_raw(&mut self, text: &str) -> Result<EmbeddingResult, String> {
        // Tokenize first to size the context (n_ubatch must be >= n_tokens
        // for encoder models).
        let tokens = self
            .model
            .str_to_token(text, AddBos::Always)
            .map_err(|e| format!("Failed to tokenize text: {e}"))?;

        if tokens.is_empty() {
            return Err("Empty token sequence".to_string());
        }

        // Pad for the BOS token and keep a sane floor.
        let n_ctx = std::cmp::max(tokens.len() + 64, 512);
        let ctx_params = LlamaContextParams::default()
            .with_embeddings(true)
            .with_n_ctx(std::num::NonZero::new(n_ctx as u32))
            .with_n_batch(n_ctx as u32)
            .with_n_ubatch(n_ctx as u32);

        let mut ctx = self
            .model
            .new_context(&self.backend, ctx_params)
            .map_err(|e| format!("Failed to create context: {e}"))?;

        let mut batch = LlamaBatch::new(tokens.len(), 1);
        for (i, token) in tokens.iter().enumerate() {
            let is_last = i == tokens.len() - 1;
            batch
                .add(*token, i as i32, &[0], is_last)
                .map_err(|e| format!("Failed to add token to batch: {e}"))?;
        }

        ctx.decode(&mut batch)
            .map_err(|e| format!("Failed to decode batch: {e}"))?;

        let embeddings = ctx
            .embeddings_seq_ith(0)
            .map_err(|e| format!("Failed to get embeddings: {e}"))?;

        if self.dimensions.is_none() {
            self.dimensions = Some(embeddings.len());
        }

        Ok(EmbeddingResult {
            embedding: embeddings.to_vec(),
            model: DEFAULT_EMBED_MODEL.to_string(),
        })
    }

    /// The embedding dimensions, known after the first embed.
    #[must_use]
    pub const fn dimensions(&self) -> Option<usize> {
        self.dimensions
    }
}

/// Document template: the title rides with the text so a chunk lifted out of
/// the middle of a document still knows what it belongs to. Must stay
/// byte-identical to what produced the existing vectors.
#[must_use]
pub fn format_doc_for_embedding(text: &str, title: Option<&str>) -> String {
    let title_str = title.unwrap_or("none");
    format!("title: {title_str} | text: {text}")
}

/// Query template (nomic-style). Must stay byte-identical: queries and
/// documents live in the same embedding space only if both sides keep the
/// template.
#[must_use]
pub fn format_query_for_embedding(query: &str) -> String {
    format!("task: search result | query: {query}")
}

// ---------------------------------------------------------------------------
// Model download
// ---------------------------------------------------------------------------

/// Model cache directory. Deliberately qmd's path: installs upgrading from a
/// qmd-built release already have the ~300MB model here, and re-downloading
/// it would be pure waste.
fn model_cache_dir() -> PathBuf {
    let cache_dir = dirs::cache_dir().unwrap_or_else(|| PathBuf::from("."));
    let model_dir = cache_dir.join("qmd").join("models");
    let _ = std::fs::create_dir_all(&model_dir);
    model_dir
}

struct HfRef {
    repo: String,
    file: String,
}

/// Parse a HuggingFace URI like "hf:user/repo/file.gguf".
fn parse_hf_uri(uri: &str) -> Option<HfRef> {
    let without_prefix = uri.strip_prefix("hf:")?;
    let parts: Vec<&str> = without_prefix.splitn(3, '/').collect();
    if parts.len() < 3 {
        return None;
    }
    Some(HfRef {
        repo: format!("{}/{}", parts[0], parts[1]),
        file: parts[2].to_string(),
    })
}

/// Remote ETag for cache validation.
fn get_remote_etag(hf: &HfRef) -> Option<String> {
    let url = format!(
        "https://huggingface.co/{}/resolve/main/{}",
        hf.repo, hf.file
    );

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;

    let resp = client.head(&url).send().ok()?;
    if !resp.status().is_success() {
        return None;
    }

    resp.headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim_matches('"').to_string())
}

/// Download a model file from HuggingFace, streaming to disk.
fn download_from_hf(hf: &HfRef, local_path: &Path, etag_path: &Path) -> Result<(), String> {
    use std::io::{Read, Write};

    let url = format!(
        "https://huggingface.co/{}/resolve/main/{}",
        hf.repo, hf.file
    );

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(3600))
        .build()
        .map_err(|e| format!("Failed to build download client: {e}"))?;

    let mut resp = client
        .get(&url)
        .send()
        .map_err(|e| format!("Failed to download {url}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Failed to download {url}: HTTP {}", resp.status()));
    }

    let total_size = resp.content_length().unwrap_or(0);
    tracing::info!("Downloading {} ({} MB)", hf.file, total_size / 1_048_576);

    let mut file = std::fs::File::create(local_path)
        .map_err(|e| format!("Failed to create {}: {e}", local_path.display()))?;
    let mut downloaded: u64 = 0;
    let mut buffer = [0u8; 8192];
    let mut last_log_mb: u64 = 0;

    loop {
        let bytes_read = resp
            .read(&mut buffer)
            .map_err(|e| format!("Download read failed: {e}"))?;
        if bytes_read == 0 {
            break;
        }
        file.write_all(&buffer[..bytes_read])
            .map_err(|e| format!("Download write failed: {e}"))?;
        downloaded += bytes_read as u64;

        // Progress via tracing every 25 MB; stdio is suppressed by the
        // caller while the TUI owns the terminal.
        let mb = downloaded / 1_048_576;
        if mb / 25 > last_log_mb / 25 {
            last_log_mb = mb;
            if total_size > 0 {
                tracing::info!("Model download: {} / {} MB", mb, total_size / 1_048_576);
            }
        }
    }

    // Save ETag for cache validation on next pull.
    if let Some(etag) = resp.headers().get("etag")
        && let Ok(etag_str) = etag.to_str()
    {
        let _ = std::fs::write(etag_path, etag_str.trim_matches('"'));
    }

    tracing::info!("Downloaded {} ({} MB)", hf.file, downloaded / 1_048_576);
    Ok(())
}

/// Pull a model: download if missing or if the remote ETag moved.
///
/// `refresh = true` forces a re-download. Returns the local path either way.
pub fn pull_model(model_uri: &str, refresh: bool) -> Result<PullResult, String> {
    let cache_dir = model_cache_dir();

    let hf_ref = parse_hf_uri(model_uri);
    let filename = match hf_ref {
        Some(ref hf) => hf.file.clone(),
        None => model_uri.to_string(),
    };

    let local_path = cache_dir.join(&filename);
    let etag_path = cache_dir.join(format!("{filename}.etag"));

    let should_download = refresh
        || !local_path.exists()
        || matches!(&hf_ref, Some(hf) if {
            // Check ETag for updates.
            let remote_etag = get_remote_etag(hf);
            let local_etag = std::fs::read_to_string(&etag_path).ok();
            remote_etag.is_some() && remote_etag != local_etag
        });

    if should_download {
        match hf_ref {
            Some(ref hf) => download_from_hf(hf, &local_path, &etag_path)?,
            None => {
                return Err(format!(
                    "Model not found and no HuggingFace URI provided: {model_uri}"
                ));
            }
        }
    }

    let size_bytes = std::fs::metadata(&local_path).map_or(0, |m| m.len());

    Ok(PullResult {
        model: model_uri.to_string(),
        path: local_path,
        size_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_template_is_stable() {
        // These exact bytes produced every vector currently stored.
        assert_eq!(
            format_doc_for_embedding("hello world", Some("Test Title")),
            "title: Test Title | text: hello world"
        );
        assert_eq!(
            format_doc_for_embedding("hello world", None),
            "title: none | text: hello world"
        );
    }

    #[test]
    fn query_template_is_stable() {
        assert_eq!(
            format_query_for_embedding("test query"),
            "task: search result | query: test query"
        );
    }

    #[test]
    fn parse_hf_uri_shapes() {
        let r = parse_hf_uri(DEFAULT_EMBED_MODEL_URI).unwrap();
        assert_eq!(r.repo, "ggml-org/embeddinggemma-300M-GGUF");
        assert_eq!(r.file, DEFAULT_EMBED_MODEL);
        assert!(parse_hf_uri("not-a-uri").is_none());
        assert!(parse_hf_uri("hf:only/two").is_none());
    }

    #[test]
    fn cache_dir_is_qmds() {
        // Upgrading installs must NOT re-download the model.
        let dir = model_cache_dir();
        assert!(dir.ends_with("qmd/models"), "got {}", dir.display());
    }
}
