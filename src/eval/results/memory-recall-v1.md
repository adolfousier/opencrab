# MEMORY.md recall: before / after

Dataset: `src/eval/fixtures/memory_recall.json` (`memory-recall-v1`)
Corpus: `src/eval/fixtures/memory_corpus.md`, 13 sections, synthetic
Scored by: `src/tests/memory_recall_eval_test.rs`, metrics from `src/eval/recall.rs`
Regenerate: `cargo test --all-features memory_recall_eval`

## Result

| metric | before (hit count >= 2) | after (normalized BM25 >= 0.35) |
|---|---|---|
| precision@2 | 0.625 | **0.917** |
| recall@2 | **1.000** | 0.917 |
| MRR | 0.917 | 0.917 |
| nDCG@2 | 0.938 | 0.917 |
| false-positive rate | 0.417 | **0.250** |

12 positive queries, 12 negatives.

## Reading it honestly

BM25 does **not** dominate. It gives up one answer in twelve to nearly halve the
noise and lift precision from 0.625 to 0.917. The old rule scores recall 1.000
because it answers almost everything, which is the same reason its precision is
poor and it fires on 5 of the 12 messages that wanted silence.

**The fixture under-states the improvement.** Measured against a real
156-section MEMORY.md and 437 real user messages, the old rule injected sections
on **89.5%** of messages; the shipped configuration takes that to **17.4%**. The
synthetic negatives here are shorter and cleaner than real conversational
messages, so they trigger the old rule less often (0.417 rather than 0.895).
Treat this file as a conservative floor.

Those real-corpus figures are not reproducible from this repository, by design:
the messages and the memory file that produced them cannot be committed.

## What the numbers do not cover

- **Threshold transfer.** 0.35 was calibrated on this 13-section fixture and a
  156-section real file. Untested between and beyond.
- **Non-English corpora.** The stemmer strips English-shaped suffixes. It is
  applied symmetrically to query and section, so the failure mode is a blunted
  score rather than a false match, but no non-English dataset was scored.
- **Uniform corpora.** When every section shares the query's terms, those terms
  discriminate nothing and recall stays silent. Pinned as a known limit in
  `memory_recall_test`.
- **Live behaviour.** Everything here is offline scoring. No measurement of
  whether injected sections actually changed an answer for the better.
