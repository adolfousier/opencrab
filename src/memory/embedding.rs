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
/// whole process.
///
/// Since #998 this is a backstop rather than the working limit: content is
/// chunked first, and a chunk is bounded by `CHUNK_SIZE_CHARS`, so nothing in
/// normal operation approaches it. It stays because a C-level abort cannot be
/// caught and the cost of being wrong is the process dying.
const MAX_EMBED_BYTES: usize = 32_000;

/// Split content the way it will be embedded and stored.
///
/// One vector per document was wrong twice over (#998). Anything past 32 KB got
/// a `skipped-too-large` placeholder and no vector at all, which on a real
/// workspace was a quarter of all rows including MEMORY.md, the one file memory
/// search exists to search. And everything under that limit collapsed into a
/// single averaged vector, so a document covering several topics landed as one
/// meaningless point in embedding space.
///
/// `qmd::chunk_document` was always available with a 15% overlap; the call
/// sites simply passed `seq = 0, pos = 0` and never used it.
pub(crate) fn chunks_for(body: &str) -> Vec<qmd::Chunk> {
    qmd::chunk_document(body, CHUNK_SIZE_CHARS, CHUNK_OVERLAP_CHARS)
}

/// Target chunk size in characters, and the overlap between neighbours.
///
/// Mirrors qmd's own defaults (800 tokens at roughly 4 characters per token,
/// with a 15% overlap) but declared here because qmd does not re-export the
/// character-based constants, and because these are the two numbers a
/// retrieval eval would tune. Overlap exists so a passage split across a
/// boundary is still wholly present in one chunk.
const CHUNK_SIZE_CHARS: usize = qmd::CHUNK_SIZE_TOKENS * 4;
const CHUNK_OVERLAP_CHARS: usize = qmd::CHUNK_OVERLAP_TOKENS * 4;

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
    let engine_mutex = engine_if_ready().ok_or("Embedding engine not initialized")?;
    // The title rides with every chunk: a chunk lifted out of the middle of a
    // document has no other clue what it belongs to.
    let title = Store::extract_title(body);
    // Hash of the WHOLE document. It is the foreign key to `content`; the
    // chunk is identified by `seq` alongside it.
    let hash = Store::hash_content(body);
    let now = crate::utils::string::utc_timestamp();

    for (seq, chunk) in chunks_for(body).into_iter().enumerate() {
        if chunk.text.len() > MAX_EMBED_BYTES {
            tracing::warn!(
                "Chunk {seq} still exceeds {MAX_EMBED_BYTES} bytes after chunking; skipping it"
            );
            continue;
        }

        // catch_unwind guards against Rust-side panics from llama-cpp bindings.
        // A C-level abort() cannot be caught, which is why the size check above
        // stays even though chunking should make it unreachable.
        let emb = {
            let mut engine = engine_mutex
                .lock()
                .map_err(|e| format!("Engine lock poisoned: {e}"))?;
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                engine.embed_document(&chunk.text, Some(&title))
            }))
            .map_err(|_| "llama.cpp panicked during embedding".to_string())?
            .map_err(|e| format!("Embedding failed: {e}"))?
        };

        // Store lock → insert → release, once per chunk so a long document
        // does not hold the store for the whole batch.
        store
            .lock()
            .map_err(|e| format!("Store lock poisoned: {e}"))?
            .insert_embedding(&hash, seq, chunk.pos, &emb.embedding, &emb.model, &now)
            .map_err(|e| format!("Failed to store embedding: {e}"))?;
    }

    Ok(())
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

    // Retire placeholders from the pre-chunking embedder first (#998), or the
    // documents that most need embedding stay permanently excluded below.
    match super::store::clear_skipped_placeholders() {
        Ok(0) => {}
        Ok(n) => tracing::info!("Backfill: reopened {n} previously skipped documents"),
        Err(e) => tracing::warn!("Backfill: placeholder sweep failed, continuing: {e}"),
    }

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
    let now = crate::utils::string::utc_timestamp();
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

        let title = Store::extract_title(body);
        // No size bail here any more (#998). It used to write a
        // `skipped-too-large` placeholder for anything past 32 KB, which meant
        // the largest and usually most valuable documents were the ones with no
        // vector, permanently, since the placeholder also stopped the retry.
        let mut chunks_stored = 0usize;

        for (seq, chunk) in chunks_for(body).into_iter().enumerate() {
            if chunk.text.len() > MAX_EMBED_BYTES {
                tracing::warn!(
                    "Chunk {seq} of '{path}' exceeds {MAX_EMBED_BYTES} bytes after chunking; \
                     skipping that chunk"
                );
                continue;
            }

            // Engine lock: embed one chunk → release
            // catch_unwind guards against panics from llama-cpp bindings.
            let emb = {
                let mut engine = match engine_mutex.lock() {
                    Ok(e) => e,
                    Err(_) => return,
                };
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    engine.embed_document(&chunk.text, Some(&title))
                })) {
                    Ok(result) => result.ok(),
                    Err(_) => {
                        tracing::error!(
                            "llama.cpp panicked during backfill embed of '{path}' chunk {seq}"
                        );
                        continue;
                    }
                }
            };

            // Store lock: insert embedding → release
            if let Some(emb) = emb
                && let Ok(s) = store.lock()
                && s.insert_embedding(hash, seq, chunk.pos, &emb.embedding, &emb.model, &now)
                    .is_ok()
            {
                chunks_stored += 1;
            }
        }

        if chunks_stored > 0 {
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
    let hash = Store::hash_content(body);
    let model_name = super::embedding_api_config()
        .and_then(|c| c.model)
        .unwrap_or_else(|| "api-embedding".to_string());
    let now = crate::utils::string::utc_timestamp();

    // Chunked like the local path (#998). The size bail is gone: a remote API
    // has no llama.cpp abort to guard against, and refusing large documents
    // outright was the reason the biggest ones had no vector at all.
    for (seq, chunk) in chunks_for(body).into_iter().enumerate() {
        let embedding = embed_via_api(&chunk.text).await?;
        store
            .lock()
            .map_err(|e| format!("Store lock poisoned: {e}"))?
            .insert_embedding(&hash, seq, chunk.pos, &embedding, &model_name, &now)
            .map_err(|e| format!("Failed to store API embedding: {e}"))?;
    }

    Ok(())
}

/// Embed a query via the API for vector search.
///
/// Returns the embedding vector, or Err if the API call fails.
pub async fn embed_query_api(query: &str) -> Result<Vec<f32>, String> {
    embed_via_api(query).await
}
