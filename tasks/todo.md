# Tasks

## DEFERRED: Split `src/tui/app.rs` into 5 Files

**Status:** Saved for later — pivoting to channel setup fixes

### Context
`app.rs` is 4,770 lines — the largest source file in the repo. Split into 5 cohesive files for navigability.

### File Layout

| File | ~Lines | Content |
|------|--------|---------|
| `app/mod.rs` | ~1000 | Types, `App` struct, `new()`, `initialize()`, `handle_event()`, utilities |
| `app/input.rs` | ~920 | All `handle_*_key()` methods, input history, pending-state checks |
| `app/messaging.rs` | ~1060 | Sessions, slash commands, message expansion, streaming, whispercrabs |
| `app/plan_exec.rs` | ~510 | Plan lifecycle (load, save, export, execute) |
| `app/dialogs.rs` | ~830 | Model selector, onboarding, file/dir pickers |

### Steps

- [ ] **Step 1** — Convert file to directory module (`mkdir src/tui/app/`, `mv app.rs app/mod.rs`, `cargo check`)
- [ ] **Step 2** — Fix field visibility: 14 private fields → `pub(crate)` (`intermediate_text_received`, `splash_shown_at`, `escape_pending_at`, `ctrl_c_pending_at`, `input_history`, `input_history_index`, `input_history_stash`, `cancel_token`, `shared_session_id`, `agent_service`, `session_service`, `message_service`, `plan_service`, `event_handler`, `prompt_analyzer`)
- [ ] **Step 3** — Extract `app/input.rs` (~lines 970–2089): `handle_key_event()`, `handle_chat_key()`, `handle_sessions_key()`, `handle_plan_key()`, `delete_last_word()`, `history_path()`, `load_history()`, `save_history_entry()`, `has_pending_*()` methods
- [ ] **Step 4** — Extract `app/messaging.rs` (~lines 2091–3076, 4652–4770): session CRUD, slash commands, message processing, streaming pipeline, `ensure_whispercrabs()`
- [ ] **Step 5** — Extract `app/plan_exec.rs` (~lines 3078–3583): plan lifecycle methods
- [ ] **Step 6** — Extract `app/dialogs.rs` (~lines 3829–4649): model selector, onboarding, file/dir pickers
- [ ] **Step 7** — Wire up module declarations in `mod.rs` (`mod input; mod messaging; mod plan_exec; mod dialogs;`)
- [ ] **Step 8** — Verify external consumers (`src/tui/mod.rs`, `render.rs`, `runner.rs`, `src/cli/ui.rs`)
- [ ] **Step 9** — Final verification: `cargo check`, `cargo clippy --all-features`, `cargo test`

---

## ACTIVE: Fix Channel Setup Issues

### Problems
1. **WhatsApp** — QR code not showing during onboarding
2. **Telegram** — Missing allowed list IDs / user ID prompts, no confirmation/done screen at end
