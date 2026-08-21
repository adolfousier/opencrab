# Telegram Userbot Tools (native)

Interactive Telegram tools riding the MTProto user session — a local
alternative to hosted MCP packages
([fast-mcp-telegram](https://github.com/alexeyleshchenko/fast-mcp-telegram)
and friends): no server, no bearer token, the session file never
leaves the machine. Complements the always-on watch loop (ingestion)
with on-demand reads, controlled sends, reactions, and media
downloads.

## Enable

```
opencrabs build --features telegram-userbot   # or cargo build --features telegram-userbot
```

## Login once

```
opencrabs userbot login          # QR flow (Settings → Devices → Scan QR)
opencrabs userbot login --code   # phone + code + 2FA flow
```

## Invoke a tool

Every invocation is a JSON params file — complex filters don't belong
on a command line:

```bash
cat > /tmp/read.json <<'EOF'
{ "tool": "read_chat", "chat": "@durov", "limit": 10 }
EOF
opencrabs userbot tool --params-file /tmp/read.json
```

Tool tags: `read_chat`, `search_chat`, `search_global`, `discover`,
`download`, `send_message`, `send_file`, `send_to_phone`,
`edit_message`, `react`, `raw`.

## Governance (enforced before any network touch)

Permissions are one map — `chat_permissions` under
`channels.telegram.userbot` in config.toml — binding each chat to the
actions allowed there:

```toml
[channels.telegram.userbot.chat_permissions]
"-1001234567890" = ["read", "send"]   # ingest AND interactive sends
"1997662613"     = ["read"]           # ingest only
"@wallet"        = ["read"]           # same map governs the watch loop
```

Keys are chat ids / usernames / E.164 phones — whatever form the
params will carry, matched literally. Absence of `send` anywhere =
strictly read-only tool plane.

| Class | Tools | Gate |
|---|---|---|
| Read | read_chat, search_chat, search_global, discover, download | userbot enabled |
| Outbound | send_message, send_file, send_to_phone, edit_message, react | target chat has `send` in `chat_permissions` |
| Dangerous | raw | `"confirm": true` inside the params, every invocation |

The same map governs the watch loop: a chat ingests ambient messages
only with `read`. One source of truth for both planes.

## Beyond plain sends

- **`wait_reply_secs`** (send_message): after sending, poll the chat
  for the first incoming reply — bounded at 120s. The envelope carries
  `reply` + `waited_secs`; timeout is `reply: null`, never an error.
  The hosted MCP camp cannot do this (no event plane); we poll
  in-process over the live session.
- **`schedule_unix`** (send_message): Telegram-native scheduled send,
  validated to a future timestamp within the 366-day window.
- **`download`** (download): fetch media for one message id. Explicit
  paths must be absolute under `$HOME` with no `..` components —
  traversal is refused at the boundary; defaults land under
  `~/.opencrabs/userbot_media/`.
- **Flood waits**: floods the library cannot absorb in-process
  (>60s) return `{"error":"flood_wait","retry_after_secs":N}` with
  exit 0 — re-invoke after N seconds instead of blind-retrying.

## Raw registry

`raw` invokes a curated, schema-verified set of read-only
constructors: `messages.getHistory`, `messages.getReplies`,
`messages.getForumTopics`, `contacts.resolveUsername`,
`contacts.resolvePhone`, `account.getAuthorizations`,
`users.getFullUser`. Peer-bearing methods accept a `"chat"` string in
params and resolve it locally. Unknown or mutating methods are
refused with the registry listing — deliberately not a wildcard
invoke (the hosted MCP's raw tool fails 55% in their own production
data; blind string dispatch is why).

## Coexistence with the Bot API plane

The bot listener (Bot API) and the userbot (MTProto) are separate
planes by construction: feature-gated module, separate session
artifacts, independent ChannelManager handles, and the tool plane is
a one-shot subprocess — the two never share a connection. At watch
startup, any allowlisted chat where the bot is ALSO a member logs a
prominent double-delivery warning.

## Agent surface (OpenCrabs dynamic tools)

`src/docs/tools-telegram-userbot.toml` is a drop-in replacement for
the MCP package's tools.toml: same tool names and params-file
contract, command swapped to this binary. Each description carries
its governance hint (read / outbound / dangerous). Copy its
`[[tools]]` entries into your profile's tools.toml and
`tool_manage action="reload"`.
