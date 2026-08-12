# Memory retrieval: what changed on 2026-08-12, and what it bought

Covers the arc from #995 through #1020: per-turn recall ranking, chunked
embeddings, index freshness, collection routing, and scoped search. Every
figure below was measured on a live workspace — 9 brain files, 158 daily notes,
93 session documents, 258 MB — not on a fixture.

Regenerate the ranking rows with `cargo test --all-features memory_recall_eval`.
The retrieval rows are point measurements against the live index and are dated
rather than reproducible.

## The question this set out to answer

Whether memory could earn its place without loading whole files into context.
Two distinct jobs turned out to be hiding under one name:

- **Historical** — what happened, when, in what order. Corpus: 158 daily notes.
- **Normative** — what are my rules, does one about this already exist. Corpus:
  9 brain files.

Every failure below comes from one query being served by the other corpus.

## 1. Finding a rule in a brain file

Five phrasings of the same question, against the live index.

| Query | Session start | After collection fix | After `scope="brain"` |
|---|---|---|---|
| `duplicate check` | 3 daily notes | daily note | **AGENTS.md** |
| `append rule memory` | 3 daily notes | daily note | **AGENTS.md** |
| `sharpen existing line` | AGENTS.md | daily note | **AGENTS.md** |
| `duplicate` | 3 daily notes | daily note | **AGENTS.md** |
| `writing a new rule` | AGENTS.md | daily note | **AGENTS.md** |
| **Hit rate** | **2/5** | **0/5** | **5/5** |

The middle column is not a regression. Those two early hits existed only because
brain files were leaking into the memory collection (#1018 follow-up); fixing the
routing removed the accident, and the scope replaced it with the real thing.

The failure mode mattered more than the rate: a miss returned three confident,
irrelevant results rather than nothing. An agent checking "does this rule exist"
read that as "no" and appended a duplicate (#1017).

## 2. Cost of one rule lookup

| Method | Payload | vs whole file | Accuracy |
|---|---|---|---|
| `load_brain_file("AGENTS.md")`, whole file | 33,443 chars | — | 5/5 |
| `load_brain_file` + `query` | 4,981–5,948 chars | −83% | 5/5 |
| `memory_search` `scope="brain"` | ~1,000 chars | **−97%** | 5/5 |

Roughly 8,400 tokens down to 250 for the same answer. The `~1,000` figure is
derived from the snippet path (5 results × 200 chars), not a captured tool
response; the accuracy column is measured.

This is why the duplicate-check instruction searches first and loads a full
section only on a hit — the cheap step became the accurate one, so the expensive
one is only paid when there is something to read.

## 3. Index freshness

| | Before | After |
|---|---|---|
| Brain files refreshed | startup only | on write + on search |
| Worst observed staleness | **15 hours** | current |
| Refresh work per search | n/a | once per boot per file |
| Freshness reindexes in log | 2–3 per search, all day | 0 since restart |

The index was a boot-time snapshot. A rule written mid-session was invisible to
search until the next restart, which is exactly when a duplicate check needs it
(#1018). The first freshness implementation compared `documents.modified_at`, a
field never bumped on content update, so it re-declared the same files stale on
every search — fixed by tracking what the process actually indexed (#1021).

## 4. Embeddings and chunking

| | Before | After |
|---|---|---|
| Vector rows | 1,944 | 5,744 |
| `max(seq)` | 0 — nothing chunked | 283 |
| Empty placeholder vectors | 495 | 0 |
| Multi-chunk documents | 0 | 95 |

A quarter of vector rows were placeholders holding no vector, so those documents
were invisible to the semantic half of hybrid search. Nothing had ever been
chunked, so anything past the size guard was skipped entirely (#998, #1000,
#1001, #1002).

## 5. Per-turn recall ranking

| Metric | Before (hit count ≥ 2) | After (normalized BM25 ≥ 0.35) |
|---|---|---|
| precision@2 | 0.625 | **0.917** |
| recall@2 | 1.000 | 0.917 |
| Multilingual precision/recall@2 | — | 1.000 / 1.000, 3/3 in all six languages |

Recall falling is the point. The old rule scored 1.000 by answering almost
everything, which is the same reason its precision was 0.625 (#996).

## 6. End-to-end validation: recovering a fact from ~2 months back

The retrieval numbers above are synthetic queries against an index. This is the
behaviour they were for.

**Task.** Audit why a monthly invoice cron job had not run on schedule — it
should fire on the 5th, June was run manually after failing, August failed too.
Read-only audit, no fixes.

**Run.** 9 steps, 10 tool calls, 1 failed, 1m 40s.

**What it did.** Opened with `memory_search`, and the search returned the setup
trail: the job was created around **19–20 June 2026**, roughly 54 days before the
query. It then read `memory/2026-06-20.md` directly, grepped the 19th and 20th
for `invoice`, and reconstructed the original setup — including that the OAuth
app was in Google's testing mode at creation time, which matters because
testing-mode refresh tokens expire on a fixed schedule.

**What made the conclusion possible.** It noticed the job ID recorded in June
differed from the current one, inferring the job had been recreated at some
point. It then found July's logs had aged out of retention and fell back to the
`cron_job_runs` table for the run history rather than reporting the gap as
unknown.

**Why this is the right test.** Nothing about it is a keyword lookup. The chain
was: search finds the era → read the specific day → grep the neighbouring day →
notice an ID discrepancy across two months → route around missing logs. The
search only had to put it in the right week; everything after that was the agent
reading primary sources it had located.

It also exercises the split from §1 in the opposite direction: this is the
historical corpus doing what it is for, where 158 daily notes are the asset
rather than the noise. The same volume that buried a rule lookup is what made a
two-month-old fact recoverable.

## What this says about the two tools

| | `memory_search` (default `memory`) | `memory_search` `scope="brain"` | `load_brain_file` + `query` |
|---|---|---|---|
| Corpus | 158 daily notes | 9 brain files | one named file |
| Answers | what happened, when | does a rule exist, where | what exactly does it say |
| Rule lookup | 0/5 | 5/5 | 5/5 |
| Cost | ~1,000 chars | ~1,000 chars | ~5,000 chars |

They are not competing implementations of one capability. Search locates; load
reads. Pointing a rule question at the historical corpus was the single largest
source of wrong answers measured here, and an empty result in the default scope
now says so explicitly rather than returning confident noise.

## Limits and what was not measured

- Every retrieval figure comes from querying the index directly, not from a live
  tool invocation inside a session. The cron audit is the only end-to-end
  evidence here, and it is one run.
- The `~1,000` payload is derived from the snippet code path, not captured.
- The 0.35 recall threshold was calibrated on a 13-section fixture; transfer to
  much larger files is untested and is pinned as a known limit in
  `memory_recall_test`.
- No latency comparison worth reporting: the FTS query measured at 6 ms and a
  file read is the same order, so the difference is cost, not speed.
- Semantic (vector) retrieval was not isolated from FTS in any of these runs.
