---
name: dynamic-tools
description: "Runtime tool management with tool_manage and tools.toml format. Create, enable, disable, reload tools without restart. (/dynamic-tools, tool_manage, runtime tools)"
---

# Dynamic Tools Reference

Runtime tool creation — define tools in `~/.opencrabs/tools.toml` and they become callable immediately without restart or rebuild.

## tool_manage Meta-Tool

| Action | Required | What |
|--------|----------|------|
| `list` | — | Show all dynamic tools with enabled/disabled status |
| `add` | `name`, `description`, `executor`, `method`/`command` | Create new tool (persists to tools.toml) |
| `remove` | `name` | Delete a tool |
| `enable` / `disable` | `name` | Toggle without removing |
| `reload` | — | Hot-reload from tools.toml |

## Executor Types

Two executor types, set via the `executor` field:

| Executor | Field |
|----------|-------|
| `executor = "shell"` | `command` — shell command string |
| `executor = "http"` | `method`, `url` — HTTP method and endpoint |

## Parameter Handling

Every tool parameter defined in `[[tools.params]]` becomes available in two ways:

1. **`$OPENCRABS_PARAMS`** — Path to a temporary JSON file containing **all** params with their current values. The file is created before the command runs and cleaned up after. Use with `--params-file` flags:

   ```toml
   command = "/usr/local/bin/tg-mcp-call send_message --params-file \"$OPENCRABS_PARAMS\""
   ```

2. **`{{param_name}}`** — Inline template substitution. Replaced with the string value of the parameter:

   ```toml
   command = "curl -s -H \"Authorization: Bearer {{api_key}}\" {{endpoint}}"
   ```

### Conditional Sections

Wrap template content with `{{#param_name}}...{{/param_name}}` to include only when the parameter is non-null and non-empty:

```toml
command = """curl -s {{#query}}-G --data-urlencode "q={{query}}"{{/query}} {{url}}"""
```

## Coercion Rules

Each param can specify how null/empty values are handled before being written to `$OPENCRABS_PARAMS`:

| `coerce_empty_to` / `coerce_null_to` | Effect |
|---------------------------------------|--------|
| `"keep"` (default) | Pass null/empty as-is into the JSON |
| `"null"` | Convert to JSON `null` |
| `"error"` | Reject the tool call with a validation error |

Example:
```toml
[[tools.params]]
name = "message"
type = "string"
required = false
default = "null"
coerce_empty_to = "null"
```

## tools.toml Format

### Shell Executor

```toml
[[tools]]
name = "tg_send_message"
description = "Send a message to a Telegram chat"
executor = "shell"
enabled = true
requires_approval = true
timeout_secs = 30
command = "/usr/local/bin/tg-mcp-call send_message --params-file \"$OPENCRABS_PARAMS\""

[[tools.params]]
name = "chat_id"
type = "string"
description = "Target chat: numeric id, username without @, or 'me'"
required = true

[[tools.params]]
name = "message"
type = "string"
description = "Message text (markdown supported)"
required = true

[[tools.params]]
name = "reply_to_id"
type = "integer"
description = "Message id to reply to"
required = false
default = "null"
```

### HTTP Executor

```toml
[[tools]]
name = "check_api_health"
description = "Check if the production API is responding"
executor = "http"
enabled = true
method = "GET"
url = "https://api.example.com/health"

[tools.headers]
Authorization = "Bearer {{api_key}}"
```

### Rate Limiting

```toml
[tools.rate_limit]
requests = "10"
window = "1m"
```

## Full Example — tg-mcp-call with `$OPENCRABS_PARAMS`

```toml
[[tools]]
name = "tg_get_messages"
description = "Fetch messages from a Telegram chat"
executor = "shell"
enabled = true
requires_approval = false
timeout_secs = 60
command = "/usr/local/bin/tg-mcp-call get_messages --params-file \"$OPENCRABS_PARAMS\" 2>&1"

[[tools.params]]
name = "chat_id"
type = "string"
description = "Target chat"
required = true

[[tools.params]]
name = "limit"
type = "integer"
description = "Max messages to return"
required = false
default = "50"

[[tools.params]]
name = "query"
type = "string"
description = "Search text"
required = false
default = ""

[[tools.params]]
name = "message_ids"
type = "string"
description = "JSON array of message ids to fetch"
required = false
default = "null"
```

## How It Works

1. On startup, tools from `tools.toml` load into the `ToolRegistry` alongside compiled tools.
2. Dynamic tools appear in the LLM's tool list — the agent calls them autonomously.
3. Use `tool_manage add` to create new tools at runtime.
4. Use `tool_manage reload` to pick up manual edits to `tools.toml`.
5. Unlike `commands.toml` (user-triggered slash commands), these are **agent-callable tools**.
6. Shell commands run via `sh -c` — `$OPENCRABS_PARAMS` is substituted at runtime with the real temp file path. The file is cleaned up after command completion.
