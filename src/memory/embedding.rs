//! Embedding — singleton engine, generate and store vector embeddings.
//!
//! Two embedding backends:
//! - **Local GGUF** (default): downloads embeddinggemma-300M (~300MB), runs via llama.cpp
//! - **API** (`[memory.embedding]` config): calls OpenAI-compatible `/v1/embeddings` endpoint
//!
//! The API path eliminates the model download and ~2.9GB RAM overhead.

use once_cell::sync::OnceCell;
use qmd::{EmbeddingEngine, Store, pull_model};
use std::sync::Mutex;

static ENGINE: OnceCell<Mutex<EmbeddingEngine>> = OnceCell::new();

/// Disable llama.cpp's C-level logging globally.
///
/// Must be called once before creating any EmbeddingEngine.
/// Routes all llama.cpp log output through the tracing framework
/// with logging disabled — zero stderr pollution.
fn silence_llama_logs() {
    use llama_cpp_2::{LogOptions, send_logs_to_tracing};
    send_logs_to_tracing(LogOptions::default().with_logs_enabled(false));
}

/// Get (or create) the shared embedding engine.
///
/// Downloads the embeddinggemma-300M model (~300MB) on first call.
/// Returns Err if the download fails (e.g. no internet) or if the CPU lacks
/// AVX (required by llama.cpp GGUF inference) — callers fall back to FTS-only.
///
/// Returns Err immediately when:
/// - `config.memory.vector_enabled = false`
/// - `[memory.embedding]` API is configured (API path used instead)
pub fn get_engine() -> Result<&'static Mutex<EmbeddingEngine>, String> {
    if !super::vector_enabled() {
        return Err(
            "Vector embeddings disabled by config [memory].vector_enabled = false".to_string(),
        );
    }

    if super::embedding_api_configured() {
        return Err("Local engine not used: [memory.embedding] API configured".to_string());
    }

    ENGINE.get_or_try_init(|| {
        check_cpu_features()?;
        silence_llama_logs();

        // Suppress hf-hub's indicatif progress bar (stderr) and any llama.cpp /
        // kalosm-common startup prints (stdout) while the TUI owns the terminal.
        // Progress is still logged via tracing, so no UX regression.
        let _fd_guard = crate::utils::fd_suppress::suppress_stdio();

        let pull = pull_model(qmd::llm::DEFAULT_EMBED_MODEL_URI, false)
            .map_err(|e| format!("Failed to pull embedding model: {e}"))?;

        let engine = EmbeddingEngine::new(&pull.path)
            .map_err(|e| format!("Failed to init embedding engine: {e}"))?;

        tracing::info!(
            "Embedding engine ready: {} ({:.1} MB)",
            pull.model,
            pull.size_bytes as f64 / 1_048_576.0
        );
        Ok(Mutex::new(engine))
    })
}

/// Verify the CPU supports the instruction sets required by llama.cpp.
/// Returns Err on x86 without AVX; passes through on ARM/other architectures.
fn check_cpu_features() -> Result<(), String> {
    #[cfg(target_arch = "x86_64")]
    {
        if !std::arch::is_x86_feature_detected!("avx") {
            return Err(
                "CPU lacks AVX — llama.cpp GGUF inference requires AVX (Sandy Bridge 2011+). \
                 Memory search will use FTS-only."
                    .to_string(),
            );
        }
    }
    Ok(())
}

/// Returns the engine if already initialized, without triggering a download.
pub fn engine_if_ready() -> Option<&'static Mutex<EmbeddingEngine>> {
    ENGINE.get()
}

/// Max bytes we'll send to llama.cpp for embedding.  Anything larger causes
/// a native `abort()` inside ggml_backend_sched_synchronize, which kills the
/// whole process.  Must match the constant in `backfill_embeddings`.
const MAX_EMBED_BYTES: usize = 32_000;

/// Generate and store an embedding for content.
///
/// Returns an error if the body is too large or the engine fails.
/// Never panics or aborts — all llama.cpp failures are caught.
///
/// No-op when `config.memory.vector_enabled = false`.
///
/// Lock ordering: engine first (embed), then store (insert). Never both at once.
pub fn embed_content(store: &Mutex<Store>, body: &str) -> Result<(), String> {
    if !super::vector_enabled() {
        return Ok(());
    }
    if body.is_empty() {
        return Ok(());
    }
    if body.len() > MAX_EMBED_BYTES {
        return Err(format!(
            "Body too large for embedding ({} bytes, max {MAX_EMBED_BYTES})",
            body.len()
        ));
    }

    let engine_mutex = engine_if_ready().ok_or("Embedding engine not initialized")?;
    let title = Store::extract_title(body);
    let hash = Store::hash_content(body);

    // catch_unwind guards against Rust-side panics from llama-cpp bindings.
    // A C-level abort() cannot be caught, so the size guard above is critical.
    let emb = {
        let mut engine = engine_mutex
            .lock()
            .map_err(|e| format!("Engine lock poisoned: {e}"))?;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            engine.embed_document(body, Some(&title))
        }))
        .map_err(|_| "llama.cpp panicked during embedding".to_string())?
        .map_err(|e| format!("Embedding failed: {e}"))?
    };

    // Store lock → insert → release
    let now = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    store
        .lock()
        .map_err(|e| format!("Store lock poisoned: {e}"))?
        .insert_embedding(&hash, 0, 0, &emb.embedding, &emb.model, &now)
        .map_err(|e| format!("Failed to store embedding: {e}"))
}

/// Backfill embeddings for all documents that don't have one yet.
///
/// Initializes the engine (downloading the model if needed) and batch-embeds
/// any documents missing embeddings. Lock ordering: store → release → engine → release → store.
///
/// No-op when `config.memory.vector_enabled = false`.
pub(super) fn backfill_embeddings(store: &Mutex<Store>) {
    if !super::vector_enabled() {
        tracing::info!("Vector embeddings disabled — skipping backfill");
        return;
    }

    let engine_mutex = match get_engine() {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("Embedding engine unavailable, skipping backfill: {e}");
            return;
        }
    };

    // Store lock: get hashes needing embeddings → release
    let needing = match store.lock() {
        Ok(s) => s.get_hashes_needing_embedding().unwrap_or_default(),
        Err(_) => return,
    };

    if needing.is_empty() {
        return;
    }

    let count = needing.len();
    tracing::info!("Backfilling embeddings for {count} documents");

    // Process one document at a time, releasing the engine lock between each
    // so other callers (session_search, embed_content) aren't blocked for the
    // entire batch duration.
    let now = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    let mut stored = 0usize;

    for (i, (hash, path, body)) in needing.iter().enumerate() {
        tracing::info!(
            "Embedding {}/{}: path={}, body_len={}, hash={}",
            i + 1,
            count,
            path,
            body.len(),
            hash
        );

        if body.len() > MAX_EMBED_BYTES {
            tracing::warn!(
                "Skipping embedding for '{}' — body too large ({} bytes, max {}). \
                 Inserting zero-vector placeholder so it won't retry.",
                path,
                body.len(),
                MAX_EMBED_BYTES
            );
            // Insert a zero-length placeholder embedding so this doc is no longer
            // returned by get_hashes_needing_embedding on every startup.
            if let Ok(s) = store.lock() {
                let _ = s.insert_embedding(hash, 0, 0, &[], "skipped-too-large", &now);
            }
            continue;
        }

        let title = Store::extract_title(body);

        // Engine lock: embed single document → release
        // catch_unwind guards against panics from llama-cpp bindings.
        let emb = {
            let mut engine = match engine_mutex.lock() {
                Ok(e) => e,
                Err(_) => return,
            };
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                engine.embed_document(body, Some(&title))
            })) {
                Ok(result) => result.ok(),
                Err(_) => {
                    tracing::error!("llama.cpp panicked during backfill embed of '{path}'");
                    continue;
                }
            }
        };

        // Store lock: insert embedding → release
        if let Some(emb) = emb
            && let Ok(s) = store.lock()
            && s.insert_embedding(hash, 0, 0, &emb.embedding, &emb.model, &now)
                .is_ok()
        {
            stored += 1;
        }
    }

    tracing::info!("Backfilled {stored}/{count} embeddings");
}

// ---------------------------------------------------------------------------
// OpenAI-compatible embedding API
// ---------------------------------------------------------------------------

/// Response from an OpenAI-compatible `/v1/embeddings` call.
#[derive(Debug, serde::Deserialize)]
struct EmbeddingApiResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Debug, serde::Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

/// Call an OpenAI-compatible embedding API to generate a vector.
///
/// Sends `POST <url>/embeddings` with `{ model, input }` and returns the
/// embedding vector. Supports OpenAI, Ollama, LM Studio, any `/v1/embeddings`.
pub async fn embed_via_api(text: &str) -> Result<Vec<f32>, String> {
    let cfg = super::embedding_api_config().ok_or("No [memory.embedding] config")?;
    let url = cfg.url.as_ref().ok_or("embedding.url not set")?;
    let model = cfg.model.as_ref().ok_or("embedding.model not set")?;

    let endpoint = if url.ends_with("/embeddings") {
        url.clone()
    } else if url.ends_with('/') {
        format!("{}embeddings", url)
    } else {
        format!("{}/embeddings", url)
    };

    let mut body = serde_json::json!({
        "model": model,
        "input": text,
    });

    // OpenAI text-embedding-3-small/large support a dimensions parameter
    if let Some(dims) = cfg.dimensions {
        body["dimensions"] = serde_json::json!(dims);
    }

    let client = reqwest::Client::new();
    let mut request = client.post(&endpoint).json(&body);

    if let Some(ref key) = cfg.api_key {
        request = request.header("Authorization", format!("Bearer {key}"));
    }

    let resp = request
        .send()
        .await
        .map_err(|e| format!("Embedding API request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Embedding API error {status}: {body}"));
    }

    let api_resp: EmbeddingApiResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to decode embedding API response: {e}"))?;

    api_resp
        .data
        .into_iter()
        .next()
        .map(|d| d.embedding)
        .ok_or_else(|| "Embedding API returned no data".to_string())
}

/// Embed content via the API and store in the qmd database.
///
/// Async counterpart of `embed_content` for the API path.
pub async fn embed_content_api(store: &'static Mutex<Store>, body: &str) -> Result<(), String> {
    if body.is_empty() {
        return Ok(());
    }
    if body.len() > MAX_EMBED_BYTES {
        return Err(format!(
            "Body too large for embedding ({} bytes, max {MAX_EMBED_BYTES})",
            body.len()
        ));
    }

    let embedding = embed_via_api(body).await?;

    let _title = Store::extract_title(body);
    let hash = Store::hash_content(body);
    let model_name = super::embedding_api_config()
        .and_then(|c| c.model)
        .unwrap_or_else(|| "api-embedding".to_string());
    let now = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();

    store
        .lock()
        .map_err(|e| format!("Store lock poisoned: {e}"))?
        .insert_embedding(&hash, 0, 0, &embedding, &model_name, &now)
        .map_err(|e| format!("Failed to store API embedding: {e}"))
}

/// Embed a query via the API for vector search.
///
/// Returns the embedding vector, or Err if the API call fails.
pub async fn embed_query_api(query: &str) -> Result<Vec<f32>, String> {
    embed_via_api(query).await
}
