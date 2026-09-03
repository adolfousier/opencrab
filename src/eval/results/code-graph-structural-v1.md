# Structural code search: before / after (symbol graph)

Corpus: this repository, 1,383 `.rs` files under the repo root
Ground truth: locked by `grep`/`rg` **before** any blind search was run
Regenerate graph: `cargo test --all-features populate_symbol_graph -- --ignored` (then probe via `memory_search`)
Feature: `code-graph` (tree-sitter + tree-sitter-rust), on by default since #1324

## Result

| Query class | Example | Before (FTS5+vector only) | After (+ symbol graph) |
|---|---|---|---|
| Callers of a function | who calls `validate_input` | text chunks, no caller info | exact callers, file + line |
| Callers, generic-heavy path | who calls `retry_db_operation` | text chunks | **5/5** callers, file + line |
| Duplicate implementations | find all HTML-escape variants | 2 of 3 found | **3 of 3**, exact locations |
| Structure of a module | what's in `tool_loop.rs` | file content only | + full function inventory |
| Concept lookup | "context compaction" | strong | unchanged — text lane untouched |

Graph after extraction fixes: **15,769 symbols, 95,601 call edges, 5,649 imports**, one-off
index of the full repo in **12.1 s**; the freshness sweep (default 300 s) keeps it current.

Ground truth for the structural rows, locked pre-run:

- `retry_db_operation` callers: 5 total (`db_retry_test.rs`, 3 in-file call sites, `src/db/retry.rs:155`)
- HTML-escape variants: `markdown.rs:278`, `rich/render_html.rs:375`, `rich/mermaid.rs:652`
- tool loop: `src/brain/agent/service/tool_loop.rs` (module has no production symbol literally named `tool_loop`)

## Reading it honestly

The before column is not a strawman: it is the same live workspace, same queries, same
grep-locked ground truth, run through `memory_search` while the session still executed on
the pre-#1324 binary. The after column is the graph lane on the populated tables.

Three extractor defects were found **during** this benchmark and fixed before the final
numbers (all have regression tests in `src/memory/symbol_extractor.rs`):

1. Method calls were stored receiver-qualified (`store.insert_symbol(...)` → callee
   `store.insert_symbol`), so `query_callers_of("insert_symbol")` missed them.
2. Enum-variant constructors were indexed as call edges — `Some` (1,278 edges) and
   `Ok` (698) were the top "callees" in the graph. Now skipped.
3. `query_symbols_by_name` ranked test symbols (`tool_loop_test`) above production
   symbols for common names. Production paths now rank first.

The edge count dropped from 37,206 (first populate) to 35,024 (final) — that delta is
almost entirely the enum noise removed, not lost real edges.

## What the numbers do not cover

- **Caller recall in generic code — resolved.** The v1 miss (`src/db/retry.rs:155`, a
  call nested in a `.await.context()` chain inside a generic function) is captured since
  [#1328](https://github.com/adolfousier/opencrabs/pull/1328); the generic-heavy row is
  5/5. Deeper generic/monomorphization edge cases remain unmeasured, but no known miss.
- **Non-Rust languages.** Only `tree-sitter-rust` is wired. Python/TS/JS grammars are
  buildable but unextracted.
- **Blind scoring at scale.** Five query classes, one repo, one operator. The spike that
  motivated this (12 queries, memory_search 5/12 vs codegraph 8/12) was run in another
  session and is not reproducible from this file.
- **Index cost on hot paths.** 5.8 s is a cold one-off over 1,383 files. Steady-state
  sweep cost per changed file is the tree-sitter parse of that file only, which is fast,
  but was not separately timed here.
