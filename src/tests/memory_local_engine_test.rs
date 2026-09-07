//! Local memory engine.
//!
//! Moved out of `src/memory/local_engine.rs`: tests live under `src/tests/`,
//! never inline beside the logic they exercise (#1076).

use crate::memory::local_engine::*;

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

/// The embedding model must load CPU-only (#1432).
///
/// The default offloads to the GPU, and on macOS that routes the load through
/// Metal into a llama.cpp assert that calls `abort()` — no unwinding, no
/// `Result`, the whole application gone while loading an optional index. A
/// user lost their session to it and had to disable vector search entirely to
/// start the app.
///
/// The crash itself cannot be reproduced here: `abort()` would take the test
/// runner with it. So the assertion is on the parameter that keeps us off that
/// code path, which is the part a future change could silently undo.
#[test]
fn the_embedding_model_never_offloads_to_the_gpu() {
    let params = embedding_model_params();
    assert_eq!(
        params.n_gpu_layers(),
        0,
        "embedding must stay on the CPU: any GPU offload puts macOS back on the Metal \
         path whose assert aborts the process"
    );
}
