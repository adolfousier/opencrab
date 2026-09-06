# Security Policy

## Reporting a vulnerability

**Do not open a public issue for a security vulnerability.**

Report it through GitHub's private vulnerability reporting: go to the
[Security tab](https://github.com/adolfousier/opencrabs/security/advisories/new)
and open a draft advisory. That channel is private to the maintainers and
lets us work on a fix before anything is public.

If private reporting is unavailable to you, contact the maintainer listed in
`Cargo.toml` directly rather than filing publicly.

### What to include

- What the issue is, and what an attacker gains
- The version or commit you saw it on, and your platform
- Minimal steps to reproduce

**Please do not include a working exploit.** Describe the class of problem and
enough to reproduce it. If a proof of concept is genuinely needed to
demonstrate impact, say so and we will arrange it privately.

### What to expect

This is a small project, so response times are best-effort rather than
contractual. We aim to acknowledge a report within a few days, and to keep you
updated while a fix is prepared. You will be credited in the advisory unless
you would rather not be.

## Supported versions

Only the latest release receives security fixes. There are no long-term
support branches, and backports to older tags are not provided.

Updating is built in: `opencrabs evolve` fetches the latest release binary and
restarts, and runs automatically on startup and every 24h while
`[agent] auto_update = true` (the default).

## Scope

**In scope** — anything that lets an attacker do something the operator did
not intend, including:

- Leaking credentials: provider API keys, bot tokens, or anything from
  `keys.toml` reaching logs, chat messages, telemetry, or a remote endpoint
- Escaping the approval gate: executing tools or shell commands the user did
  not approve, or a plan activating without consent
- Crossing a session or channel boundary: reading or writing another session's
  context, or delivering to a chat that was never bound
- Remote input causing local execution: a channel message, tool result, or
  fetched page leading to command execution
- Path escapes in file tools, or in the drag-and-drop transfer path

**Out of scope**

- Anything requiring the operator to deliberately configure a hostile provider
  endpoint, a malicious MCP server, or a hostile `tools.toml`. Those are
  trusted inputs by design: the agent runs what you point it at.
- The model doing something you disagree with. That is a prompting or policy
  question, not a vulnerability, unless it bypasses an approval gate.
- Vulnerabilities in a provider's own API or a model's output.
- Dependency advisories that are already known and tracked (see below).

## Known and tracked

Some advisories in the dependency tree are known, recorded with their
reasoning, and suppressed deliberately in CI rather than by accident. They are
annotated in `.github/workflows/ci.yml` with why each one stands and what
would clear it.

Before reporting a dependency advisory, please check that list and
[#1402](https://github.com/adolfousier/opencrabs/issues/1402). A report that
one of them is *reachable in a way we have not accounted for* is very welcome
and is not a duplicate.

## A note on the other SECURITY.md

`src/docs/reference/templates/SECURITY.md` is a different document with the
same name. It is a **brain file**: instructions the agent itself follows about
third-party code review, network posture, and credential handling, seeded into
`~/.opencrabs/` on install and owned by the user afterwards.

It governs how the agent behaves. This file is how you report a vulnerability
in OpenCrabs. They are not substitutes for each other.
