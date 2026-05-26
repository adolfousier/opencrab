# Telegram session simulator

Fast TDD harness for OpenCrabs Telegram session routing ([issue #121](https://github.com/adolfousier/opencrabs/issues/121)) without compiling Rust locally.

## Run (proper TDD)

Contract tests assert **correct** behavior and are parametrized as `[production]` vs `[fixed]`:

```bash
cd tools/telegram_session_sim

# Full suite: production params FAIL (red), fixed params PASS (green), units PASS
python3 -m pytest -q

# While developing the fix — only the green path
python3 -m pytest -q -k fixed

# Prove production still has the bug (all red on contract tests)
python3 -m pytest -q -k production
```

Expected on current code before upstream handler matches `fixed`:

| Command | Result |
|---------|--------|
| `pytest -q` | 3 failed `[production]`, 3 passed `[fixed]`, 6 unit tests passed |
| `pytest -q -k fixed` | all green |
| `pytest -q -k production` | 3 failed (intentional red) |

When the Rust fix is ported and the sim’s `production` resolver is updated to match, `pytest -q` goes fully green.

## What it models

- Suffix lookup (`[chat:ID]` → newest `updated_at`)
- Label drift (buggy: full-title compare; fixed: `should_refresh_label`)
- Auto-title composition (preserve `[chat:ID]`)
- `/sessions` switch via `chat_sessions` map (fixed resolver only)

## Resolvers

| Param id | `use_fixed_resolver` | Role in TDD |
|----------|----------------------|-------------|
| `production` | `False` | Red until bug fixed (`resolve_suffix_only` + naive label drift) |
| `fixed` | `True` | Green target (`resolve_with_chat_map` + `should_refresh_label`) |

Upstream Rust tests: `src/tests/telegram_session_resolve_test.rs` and `src/channels/telegram/session_resolve.rs`.

## Hermes validation (manual)

After deploying a build with the fix:

1. `/new` → send a message → wait ~10s for auto-title.
2. Send a **second** message — title in `/sessions` must stay non-default.
3. `/sessions` → switch an older session → send `ping` — reply should use that session's context.

```bash
ssh hermes 'sqlite3 ~/.opencrabs/profiles/ops/opencrabs.db \
  "SELECT substr(title,1,60), auto_title_attempted, updated_at FROM sessions \
   WHERE title LIKE \"%Telegram%\" ORDER BY updated_at DESC LIMIT 5;"'
```
