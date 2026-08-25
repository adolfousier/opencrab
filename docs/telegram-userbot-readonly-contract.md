# Telegram Userbot Read-Only PR Contract

## Problem

PR #1113 grew beyond its stated receive-only contract. It now combines authentication, ambient ingestion, interactive reads, account mutation, media I/O, and raw MTProto access. That scope is difficult to review and misrepresents the trust boundary.

## Target state

This branch adds only a locally authenticated, receive-only Telegram userbot ingestion plane.

## Included

1. Cargo feature gating and the minimum gramers dependencies.
2. QR/code/2FA login with a local session file written atomically and permissioned `0600` on Unix.
3. A receive-only update loop.
4. A pre-conversion, pre-storage chat allowlist; empty means dry mode.
5. Independent ChannelManager lifecycle/reconciliation.
6. Own-message, via-bot, and bot-sender loop prevention.
7. Focused configuration, login, session, reconciliation, and watch tests.

## Explicit non-goals

- No `userbot/tools/` module or dynamic tool definitions.
- No message send, edit, reaction, scheduling, wait-for-reply, or send-to-phone path.
- No media download or filesystem output surface.
- No raw MTProto invocation registry.
- No outbound/write permission model.
- No unrelated CI, lint, WhatsApp, A2A, or sub-agent cleanup.

## Verification

The final diff must compile and pass focused tests with `telegram-userbot`, contain no account-mutating or raw-tool implementation, and pass the repository CI-equivalent lint/test gates where runner infrastructure permits.

## Simplicity decision

Use one read allowlist because this PR has one capability: receive. A permission lattice would encode capabilities that deliberately do not exist and would therefore complect this trust boundary with future work.
