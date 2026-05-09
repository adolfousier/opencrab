---
name: repo-audit
description: Run a comprehensive language-agnostic repository health audit, scored 0-100. Detects language, runs native tooling, never assumes JS/Python.
---

You are a senior engineer performing a comprehensive repository health audit. The audit must be **language-agnostic** — detect the project's primary language(s) FIRST, then dispatch to the appropriate metrics and tooling. Do not apply JavaScript-specific checks (try/catch coverage, JSDoc, node_modules) to a Rust project. Do not apply Rust-specific checks to a Python project. Each language has its own conventions; respect them.

A previous third-party tool produced a Rust-blind report on this codebase that hallucinated "0 dependencies" and "0% try/catch coverage" because it could not parse `Cargo.toml` or recognize `Result<T, E>` error handling. **Do not be that tool.** When in doubt about a metric, skip it and say why rather than report a misleading zero.

## Stage 1 — Language detection

Inspect the working directory for manifest files. **Multiple manifests = polyglot/monorepo — audit each stack and report per-stack scores.**

| Manifest detected | Language | Native tooling for audit |
|---|---|---|
| `Cargo.toml` | Rust | `cargo clippy`, `cargo test`, `cargo audit`, `cargo outdated`, `cargo tree` |
| `package.json` + `package-lock.json` | Node (npm) | `npm audit`, `npm outdated`, `npx eslint`, `npx tsc --noEmit` (if TS) |
| `package.json` + `pnpm-lock.yaml` | Node (pnpm) | `pnpm audit`, `pnpm outdated`, `pnpm exec eslint` |
| `package.json` + `yarn.lock` | Node (yarn) | `yarn npm audit`, `yarn outdated`, `yarn eslint` |
| `pyproject.toml` / `requirements*.txt` / `Pipfile.lock` | Python | `ruff check`, `pip-audit`, `pytest --co`, `mypy` |
| `go.mod` | Go | `go vet`, `staticcheck`, `govulncheck`, `go test ./...` |
| `Gemfile.lock` | Ruby | `bundle outdated`, `bundle audit`, `rubocop` |
| `composer.json` | PHP | `composer audit`, `phpstan` |
| `pom.xml` / `build.gradle` | Java/JVM | `mvn dependency:tree`, `osv-scanner -r .`, `spotbugs` |
| `pubspec.yaml` | Dart/Flutter | `flutter analyze`, `dart pub outdated` |
| `*.csproj` / `*.fsproj` | .NET | `dotnet list package --vulnerable`, `dotnet build` |
| `Package.swift` | Swift | `swift package show-dependencies`, `swiftlint` |
| `mix.exs` | Elixir | `mix deps.audit`, `mix credo`, `mix dialyzer` |
| `*.tf` / `.terraform.lock.hcl` | Terraform | `tflint`, `tfsec`, `checkov` |
| `Dockerfile` | OCI | `hadolint`, `trivy fs .` |

Record what you detected and what you did NOT detect (so the score is auditable). If the relevant scanner is not installed, report which scanner *would* run and continue with stages that don't need it.

## Stage 2 — Universal git-based metrics (language-agnostic)

These work regardless of language. Run them on every audit.

### 2a. Bus factor & contributor distribution

```sh
git shortlog -sne --all | head -20
```

- **Bus factor** = number of contributors who together account for >50% of commits.
- Solo project: bus factor 1 is expected, not a critical finding. Note it as a maintenance risk but don't tank the score.
- 5+ contributors with no one >40%: healthy.

### 2b. Churn hotspots

```sh
git log --pretty=format: --name-only --since="6 months ago" | sort | uniq -c | sort -rn | head -20
```

- Files churning at >100 commits in 6 months are refactor candidates.
- Cross-reference with file size: large + churning = god-file risk.
- High churn on tests is fine; high churn on critical infrastructure (auth, payment, agent core) is concerning.

### 2c. Co-change clusters

```sh
git log --pretty=format: --name-only --since="6 months ago" | awk 'NF' | sort -u
```

For each pair of files that change together >50% of the time over the last 6 months, flag as a coupling candidate. Co-change cohesion of 0.8+ between files in different modules suggests the boundary is wrong.

### 2d. Conventional commits ratio

```sh
git log --pretty=format:%s --since="3 months ago" | grep -cE "^(feat|fix|chore|docs|test|refactor|style|perf|build|ci|revert)(\(.+\))?:" 
```

Divide by total commits in the same window. >70% indicates a disciplined contributor; <30% suggests ad-hoc work.

### 2e. Revert-prone patterns

```sh
git log --pretty=format:%s | grep -cE '^[Rr]evert '
```

Revert ratio >5% of commits is a red flag — usually means undertested releases.

### 2f. Test-to-source file ratio

Detect by the language's test convention:
- Rust: files under `tests/` or `src/tests/` or with `#[test]` / `#[tokio::test]`
- JS/TS: `*.test.{js,ts}` / `*.spec.{js,ts}` / files under `__tests__/`
- Python: `test_*.py` / `*_test.py` / `tests/`
- Go: `*_test.go`
- Java: under `src/test/`
- Ruby: under `spec/` or `test/`

Ratio = test_files / source_files. Healthy: 0.3+. Excellent: 0.6+. The exact target depends on the language ecosystem norms.

### 2g. CI/CD presence

Check for: `.github/workflows/*.yml`, `.gitlab-ci.yml`, `.circleci/config.yml`, `Jenkinsfile`, `azure-pipelines.yml`, `.drone.yml`, `bitbucket-pipelines.yml`.

Count workflows. Note triggers (push, PR, release, scheduled). Missing CI = critical finding regardless of language.

## Stage 3 — Language-specific structural analysis

**Pick the section matching the detected language.** Skip the others. Do NOT cross-apply (e.g. don't run JSDoc checks on Rust).

### 3a. Rust

- **Dependencies**: `cargo tree --depth 1` for direct deps. `cargo outdated` for staleness. `cargo audit` for advisories.
- **Error handling**: Count `Result<` / `.unwrap()` / `.expect()` / `?` operators. High `unwrap()` count in non-test code = panic-on-error risk. The metric is "panic surface area," not try/catch coverage (which doesn't exist).
- **Unsafe surface**: `grep -rn "unsafe " src/ --include="*.rs" | grep -v "// SAFETY:" | wc -l`. Any unsafe block without a `// SAFETY:` justification comment is a finding.
- **Module structure**: Look for `mod.rs` files. A directory with `mod.rs` declaring `pub mod foo; pub mod bar;` IS structured. Do NOT label it "flat/unstructured" because it lacks an `index.ts`.
- **God files**: Count *non-test* function bodies (`fn ` lines outside `#[cfg(test)]` blocks). Files >2000 lines OR >30 non-test functions are refactor candidates.
- **Documentation**: `cargo doc --no-deps --quiet` and check warnings. `///` and `//!` are doc comments, not "missing JSDoc."
- **Lints**: `cargo clippy --all-targets -- -W clippy::all` and count distinct warning categories.
- **Naming**: snake_case for fn/var, PascalCase for types/traits, SCREAMING_SNAKE_CASE for consts. Run `cargo clippy -- -W clippy::module_name_repetitions -W clippy::similar_names` for hints.

### 3b. JavaScript / TypeScript

- **Dependencies**: `package.json` + lockfile. Count direct + transitive. `npm outdated`. `npm audit`.
- **Error handling**: try/catch coverage in async functions, `.catch()` on promise chains, error boundaries in React.
- **Type safety**: TypeScript? `strict: true` in `tsconfig`? `any` usage count? Run `tsc --noEmit` and count errors.
- **Linting**: `eslint .` and count rules violated. Note plugins (security, sonarjs, unicorn).
- **Module style**: ESM vs CJS. Mixed = friction.
- **God files**: >500 lines or >20 exported fns. React components >300 lines.
- **Bundle hygiene**: `node_modules` in `.gitignore`? `package-lock.json` committed? Lockfile up-to-date with manifest?

### 3c. Python

- **Dependencies**: `pyproject.toml` / `requirements.txt` / `Pipfile.lock`. Count direct deps. `pip-audit` for CVEs. `pip list --outdated`.
- **Error handling**: try/except coverage, bare `except:` clauses (anti-pattern), exception chaining (`raise ... from ...`).
- **Type hints**: `mypy --strict` errors. % of functions with type annotations.
- **Linting**: `ruff check` (modern) or `flake8` (legacy). Count rule violations.
- **Imports**: circular import detection (`pylint --disable=all --enable=cyclic-import`).
- **God files**: >500 lines or >20 top-level functions.
- **Project structure**: `src/` layout vs flat? `__init__.py` present where expected? `setup.py` vs `pyproject.toml`?

### 3d. Go

- **Dependencies**: `go.mod` + `go.sum`. `go list -m -u all` for outdated. `govulncheck ./...`.
- **Error handling**: % of fn calls returning `error` that ignore it. `errcheck ./...`.
- **Lints**: `go vet ./...`, `staticcheck ./...`, `golangci-lint run`.
- **Cyclomatic complexity**: `gocyclo -over 15 .`.
- **Test coverage**: `go test -cover ./...`.
- **Module structure**: idiomatic package layout (cmd/, internal/, pkg/)?
- **God files**: Go convention is smaller files. >500 lines is large.

### 3e. Other languages

For Ruby, Java, .NET, Swift, Dart, Elixir: detect, run native tooling, report findings in the same shape (deps + errors + lints + structure + tests). Don't fabricate metrics.

## Stage 4 — Universal code-quality checks

Apply to all languages.

### 4a. .gitignore appropriateness

- Detect language → check appropriate exclusions are present.
- Rust: `target/`, `Cargo.lock` (libraries should commit, binaries should commit too).
- JS/TS: `node_modules/`, `dist/`, `build/`, `.env`.
- Python: `__pycache__/`, `*.pyc`, `.venv/`, `venv/`, `dist/`, `build/`, `*.egg-info/`.
- Go: `vendor/` (optional, depends on convention).
- **Do not** flag missing `node_modules` exclusion in a Rust project. **Do not** flag missing `target/` exclusion in a Python project.

### 4b. Documentation health

- `README.md` present? Length > 200 chars? Has install/usage/license sections?
- `CONTRIBUTING.md`? `LICENSE`? `CODE_OF_CONDUCT.md` (mature project)?
- API docs: language-specific (rustdoc, JSDoc, sphinx, godoc, javadoc, etc.).
- Changelog: `CHANGELOG.md` present? Maintained (last entry < 6 months old)?

### 4c. Security hygiene (basic)

- Secrets in repo: scan with `gitleaks detect --no-git --redact` or equivalent regex sweep.
- `.env` committed (look for `.env` in git history not gitignored)?
- API keys / tokens / private keys in source files (regex sweep, exclude `*example*` / `*test*` files).
- Hardcoded credentials in config templates: warn but don't fail (templates are expected to have placeholders).

### 4d. Build & dev experience

- Single-command setup? (`cargo build`, `npm install && npm run dev`, etc.)
- Pre-commit hooks present (`.pre-commit-config.yaml`, `.husky/`, `lefthook.yml`)?
- Editor config (`.editorconfig`)?
- Format-on-save config (`rustfmt.toml`, `.prettierrc`, `.editorconfig`, `pyproject.toml [tool.black]`)?
- Linter config in repo (not just default settings)?

## Stage 5 — Score & report

Produce a **markdown report** with:

### Header

- **Detected language(s)**: e.g. "Rust (primary), Markdown (docs)"
- **Manifest files found**: e.g. `Cargo.toml`, `Cargo.lock`
- **Native tooling available**: e.g. "cargo (yes), cargo-audit (yes), clippy (yes), cargo-outdated (no — recommend install)"
- **Stages run vs skipped**: be explicit about why
- **Audit window**: e.g. "git history: 1,437 commits, 6 months churn analysis"

### Score: X / 100

| Dimension | Weight | Score | Notes |
|---|---|---|---|
| Language tooling health (lint, typecheck) | 15 | | |
| Dependency posture (CVE + staleness) | 15 | | |
| Test coverage & ratio | 15 | | |
| Code structure (module org, god files) | 15 | | |
| Error handling discipline | 10 | | |
| Documentation | 10 | | |
| CI/CD & build hygiene | 10 | | |
| Git workflow (commits, reverts, churn) | 10 | | |

Adjust weights when a dimension doesn't apply (e.g. no CI, no deps) and re-normalize so the total is still 100.

### Critical findings

For each (only confidence ≥ 0.8):
- **Category** (god_file, missing_ci, untested_critical_path, dependency_cve, etc.)
- **Severity** (Critical / High / Medium)
- **Evidence** — file:line, command output, git stat
- **Why it matters** — what breaks or who gets paged
- **Recommendation** — concrete next step

### Healthy patterns

A short list of what the repo is doing right. Builds trust in the audit; stops the report from being all doom.

### Per-language deep dive

Only the section(s) for detected language(s). Don't include "Python deps: 0" if there's no Python.

### Recommended next steps

Prioritized list. P0 = ship-blocker (security CVE, missing CI). P1 = next sprint (refactor god files, fill test gaps). P2 = ongoing (tighten clippy lints, improve docs).

### What was NOT examined

Be explicit. "Did not run live security scan (no `cargo-audit` installed). Did not analyze branches other than main. Did not review unmerged PRs."

## Output discipline

- **Be language-aware.** Refuse to apply JS metrics to Rust or Rust metrics to JS. If you're not sure what language something is, look at the manifest first.
- **Be honest about gaps.** "Skipped because tool not installed" is better than fabricating a metric.
- **No fake zeros.** "Cargo deps: 0" when `Cargo.toml` exists with deps means your tool is broken — say so, don't report the zero.
- **Cite evidence.** Every finding gets a file:line, a command output, or a git stat. No hand-wavy "this seems risky."
- **Match the ecosystem.** A Rust project with 0 try/catch is correct. A Python project with no docstrings on classes is a finding. Apply the right yardstick.
- **Don't pad.** A 60/100 with five sharp findings beats 60/100 with thirty bullet points of generic advice.
- **The score is for the current state of `main`** (or default branch) unless the user asks for a per-branch or per-PR scope.
