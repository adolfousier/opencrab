# Chunked embeddings: what changed, and what was not measured

Covers #998 (chunk before embedding, make later chunks searchable) and #1000
(narrow lexical hits to the matching chunk).

## Measured, before the fix

From a real 227 MB store:

```
total documents            260
vector rows              1,944
skipped-too-large          495   (25.5%)
max(seq)                     0   (nothing had ever been chunked)
```

Brain files, by whether they had a usable vector:

```
AGENTS.md, BOOT.md, CODE.md, HEARTBEAT.md,
SECURITY.md, SOUL.md, TOOLS.md, USER.md      embeddinggemma-300M-Q8_0.gguf
MEMORY.md                                    skipped-too-large
```

The remaining 84 placeholders were daily memory logs.

Not an embedding-setup failure: the local engine was running and embedding
everything else. `MAX_EMBED_BYTES` is 32,000, MEMORY.md is ~99 KB, and the
placeholder was written specifically so it would never retry.

## What this changes

**Coverage.** A quarter of vector rows were placeholders holding an empty
vector, so those documents were invisible to the semantic half of hybrid
search. The single file `memory_search` exists to search was among them. With
chunking no chunk approaches the size guard, so they become embeddable.

**Precision.** A document under the limit collapsed into one averaged vector,
which for a file covering several topics is close to meaningless for
similarity. Ranking is now per chunk, reduced to the best chunk per document.

**Unit agreement.** Both halves of hybrid search now rank chunks. Previously
the vector side ranked documents and so did the lexical side, and after #998
they disagreed, which RRF fuses without complaining.

## What this does NOT change: token spend

Worth stating plainly, because it is the intuitive assumption and it is wrong.

`memory_search` returns `extract_snippet(body, query, 200)`, capped at 200
characters per result. Retrieval never handed whole documents to the model, so
per-call token cost was already small and is essentially unchanged.

There is an indirect token argument: when retrieval failed to surface the right
thing, the fallback was `load_brain_file("MEMORY.md")` with no query, which
loads the entire file (~99 KB, roughly 25k tokens). Better retrieval should
mean fewer of those. That is a claim about behaviour, and it has not been
measured. It is recorded here as a hypothesis, not a result.

## Not measured

- Retrieval quality before versus after, on a real corpus. The change removes a
  structural defect (missing and averaged vectors); no benchmark has been run.
- Chunk size and overlap. These are qmd's defaults, 800 tokens with 15% overlap,
  now declared in `embedding.rs`. Both halves agreeing on the unit makes these
  the obvious thing to tune, and nothing has tuned them.
- Re-embedding cost. Clearing the placeholders makes 495 rows eligible again, so
  the next backfill does real work. Its duration was not timed.
