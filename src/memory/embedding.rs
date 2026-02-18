//! Embedding — singleton engine, generate and store vector embeddings.

use once_cell::sync::OnceCell;
use qmd::{EmbeddingEngine, Store, pull_model};
use std::sync::Mutex;

static ENGINE: OnceCell<Mutex<EmbeddingEngine>> = OnceCell::new();

/// Get (or create) the shared embedding engine.
///
/// Downloads the embeddinggemma-300M model (~300MB) on first call.
/// Returns Err if the download fails (e.g. no internet) or if the CPU lacks
/// AVX (required by llama.cpp GGUF inference) — callers fall back to FTS-only.
pub fn get_engine() -> Result<&'static Mutex<EmbeddingEngine>, String> {
    ENGINE.get_or_try_init(|| {
        check_cpu_features()?;

        let pull = pull_model(qmd::llm::DEFAULT_EMBED_MODEL_URI, false)
            .map_err(|e| format!("Failed to pull embedding model: {e}"))?;

        // Suppress llama.cpp's C-level stderr spam during model load —
        // it corrupts the TUI and breaks Ctrl+C handling.
        let saved = suppress_stderr();
        let engine = EmbeddingEngine::new(&pull.path);
        restore_stderr(saved);

        let engine = engine.map_err(|e| format!("Failed to init embedding engine: {e}"))?;
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

/// Redirect stderr to /dev/null, returning the saved fd (or -1 on failure).
#[cfg(unix)]
fn suppress_stderr() -> i32 {
    unsafe {
        let saved = libc::dup(libc::STDERR_FILENO);
        if saved >= 0 {
            let devnull = libc::open(c"/dev/null".as_ptr(), libc::O_WRONLY);
            if devnull >= 0 {
                libc::dup2(devnull, libc::STDERR_FILENO);
                libc::close(devnull);
            }
        }
        saved
    }
}

/// Restore stderr from a previously saved fd.
#[cfg(unix)]
fn restore_stderr(saved: i32) {
    if saved >= 0 {
        unsafe {
            libc::dup2(saved, libc::STDERR_FILENO);
            libc::close(saved);
        }
    }
}

#[cfg(not(unix))]
fn suppress_stderr() -> i32 { -1 }
#[cfg(not(unix))]
fn restore_stderr(_saved: i32) {}

/// Returns the engine if already initialized, without triggering a download.
pub(super) fn engine_if_ready() -> Option<&'static Mutex<EmbeddingEngine>> {
    ENGINE.get()
}

/// Generate and store an embedding for content. No-ops if engine not yet initialized.
///
/// Lock ordering: engine first (embed), then store (insert). Never both at once.
pub(super) fn embed_content(store: &Mutex<Store>, body: &str) {
    let engine_mutex = match engine_if_ready() {
        Some(e) => e,
        None => return,
    };

    let title = Store::extract_title(body);
    let hash = Store::hash_content(body);

    // Engine lock → embed → release
    let emb = match engine_mutex.lock() {
        Ok(mut engine) => match engine.embed_document(body, Some(&title)) {
            Ok(emb) => emb,
            Err(e) => {
                tracing::debug!("Embedding failed: {e}");
                return;
            }
        },
        Err(_) => return,
    };

    // Store lock → insert → release
    let now = chrono::Local::now()
        .format("%Y-%m-%dT%H:%M:%S")
        .to_string();
    if let Ok(s) = store.lock()
        && let Err(e) = s.insert_embedding(&hash, 0, 0, &emb.embedding, &emb.model, &now)
    {
        tracing::debug!("Failed to store embedding: {e}");
    }
}

/// Backfill embeddings for all documents that don't have one yet.
///
/// Initializes the engine (downloading the model if needed) and batch-embeds
/// any documents missing embeddings. Lock ordering: store → release → engine → release → store.
pub(super) fn backfill_embeddings(store: &Mutex<Store>) {
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

    let items: Vec<(String, Option<String>)> = needing
        .iter()
        .map(|(_hash, _path, body)| {
            let title = Store::extract_title(body);
            (body.clone(), Some(title))
        })
        .collect();

    // Engine lock: batch embed → release
    let results: Vec<_> = {
        let mut engine = match engine_mutex.lock() {
            Ok(e) => e,
            Err(_) => return,
        };
        engine
            .embed_batch_with_progress(&items, |done, total| {
                if done % 10 == 0 || done == total {
                    tracing::debug!("Embedding progress: {done}/{total}");
                }
            })
            .into_iter()
            .map(|r| r.ok())
            .collect()
    };

    // Store lock: insert all embeddings → release
    let now = chrono::Local::now()
        .format("%Y-%m-%dT%H:%M:%S")
        .to_string();
    let mut stored = 0usize;
    if let Ok(s) = store.lock() {
        for (i, emb) in results.iter().enumerate() {
            if let Some(emb) = emb {
                let hash = &needing[i].0;
                if s.insert_embedding(hash, 0, 0, &emb.embedding, &emb.model, &now)
                    .is_ok()
                {
                    stored += 1;
                }
            }
        }
    }

    tracing::info!("Backfilled {stored}/{count} embeddings");
}
