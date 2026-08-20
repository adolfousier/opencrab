# Telegram Userbot Tools (native)

Interactive Telegram tools riding the MTProto user session — the same
8-tool surface as the hosted MCP package
([fast-mcp-telegram](https://github.com/alexeyleshchenko/fast-mcp-telegram)),
but local: no server, no bearer token, the session file never leaves
the machine. Complements the always-on watch loop (ingestion) with
on-demand reads and controlled sends.

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
`send_message`, `send_file`, `send_to_phone`, `edit_message`, `raw`.

## Governance (enforced before any network touch)

| Class | Tools | Gate |
|---|---|---|
| Read | read_chat, search_chat, search_global, discover | userbot enabled |
| Outbound | send_message, send_file, send_to_phone, edit_message | target in `outbound_allowlist` (empty list = strictly read-only) |
| Dangerous | raw | `"confirm": true` inside the params, every invocation |

`outbound_allowlist` lives under `channels.telegram.userbot` in
config.toml and holds chat ids / usernames / E.164 phones — whatever
form the params will carry, matched literally.

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

## Agent surface (OpenCrabs dynamic tools)

`src/docs/tools-telegram-userbot.toml` is a drop-in replacement for
the MCP package's tools.toml: same tool names and params-file
contract, command swapped to this binary. Copy its `[[tools]]`
entries into your profile's tools.toml and `tool_manage
action="reload"`.
