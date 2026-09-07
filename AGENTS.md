# AGENTS.md — OpenCrabs repository directives

Directive file for AI coding agents working in this repo (and a fast index for humans).

## Read first

1. **[Ontology](src/docs/reference/ONTOLOGY.md)** — the project's shared vocabulary (SSOT: one concept = one name). Use these exact terms in code, docs, issues, and PRs. New terms go through the ontology.
2. [Brain Constitution](src/docs/reference/BRAIN_CONSTITUTION.md) — canonical reference for brain files, directives, prompt assembly (AS IS policy).
3. [CONTRIBUTING.md](CONTRIBUTING.md) — PR requirements and CI gates.
4. [TESTING.md](TESTING.md) — test conventions and the test battery.

## Project layout (map to ontology terms)

| Path | What lives here |
|---|---|
| `src/brain/` | The intelligence layer: LLM providers, agent services, tools, prompt assembly |
| `src/channels/` | Channel integrations (Telegram, Discord, Slack, WhatsApp) |
| `src/memory/` | Memory store (SQLite FTS5 + vector search) |
| `src/cron/` | Cron scheduler |
| `src/tui/` | Terminal UI |
| `src/config/`, `src/db/`, `src/utils/` | Config, database, shared utilities |
| `src/docs/` | User and developer documentation (incl. `reference/ONTOLOGY.md`) |

## Conventions

- **One concept = one name.** Terms defined in `src/docs/reference/ONTOLOGY.md` are used verbatim; do not invent synonyms. If a PR introduces or renames a concept, update the ontology in the same PR.
- **Docs are AS IS.** Documentation describes what the code does, not what it should do — desired behavior goes to issues (see Brain Constitution's standing rule).
- Rust code follows the CI gates in `CONTRIBUTING.md` (fmt, clippy `-D warnings`, all-features tests).
