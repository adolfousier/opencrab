---
name: multi-agent
description: "Orchestrate a task across multiple AI models/providers. Define a team of agents with different roles, providers, and models. Each agent handles a phase (planning, execution, auditing, etc.) and results are combined. (/multi-agent, orchestrate, multi-model, team)"
---

# Multi-Agent Orchestration

You are orchestrating a task across a team of AI agents. Each team member has a **role**, a **provider**, and a **model**. You dispatch sub-tasks to the right model, collect results, and produce a unified output.

## How It Works

When the user provides a task, follow this flow:

### Phase 1 — Define the Team

If the user provides a team definition, use it. If not, propose a default team based on the task type. The team definition is a list of agents:

```yaml
team:
  - role: planner
    provider: anthropic
    model: claude-opus-4-20250514
    task: "Analyze the request and produce a structured plan with clear steps, dependencies, and risks."
  - role: executor
    provider: openrouter
    model: google/gemini-2.5-pro
    task: "Execute each step of the plan. Write code, make changes, run commands."
  - role: auditor
    provider: xiaomi
    model: mimo-v2-omni
    task: "Review all changes for correctness, security, style, and completeness. Report issues."
```

Present the team to the user for confirmation before proceeding. The user can:
- Change providers/models for any role
- Add or remove roles
- Adjust the task description for each role
- Skip confirmation with "go" or "just do it"

### Phase 2 — Execute Sequentially

Run each agent in order. For each agent:

1. **Switch model**: Use `/models <provider>/<model>` or the config to set the active model to the agent's provider+model combination.

2. **Dispatch the task**: Send the agent's `task` as a prompt, along with:
   - The original user request
   - Any output from previous agents (context handoff)
   - Specific instructions for this phase

3. **Collect output**: Capture the full response. This becomes input for the next agent.

4. **Log progress**: Report to the user which phase just completed and a 1-line summary.

### Phase 3 — Synthesize

After all agents have run:

1. Combine all outputs into a coherent result
2. Highlight any conflicts or issues found by the auditor
3. Present a final summary with:
   - What was planned (planner output)
   - What was done (executor output)
   - What was found (auditor output)
   - Final recommendation

## Provider/Model Reference

When the user doesn't specify models, suggest from these based on the task:

| Role | Recommended Provider | Model | Why |
|------|---------------------|-------|-----|
| **Planner** (strategy, architecture) | anthropic | claude-opus-4-20250514 | Deep reasoning, structured thinking |
| **Executor** (code, implementation) | openrouter | anthropic/claude-sonnet-4 | Fast, good code quality |
| **Auditor** (review, security) | openrouter | google/gemini-2.5-pro | Large context, good at catching details |
| **Researcher** (web, docs) | brave/web_search | — | Built-in search tools |
| **Writer** (docs, content) | openrouter | openai/gpt-4.1 | Clear, concise writing |
| **Quick tasks** (formatting, small fixes) | xiaomi | mimo-v2-omni | Fast, lightweight |

The user can override any of these. Ask which providers they have configured.

## Dispatch Methods

To actually call a different model, use one of:

1. **Model switch** (preferred): Use `/models <provider>/<model>` to switch the active model, then send the task as your next prompt. The model switch is session-scoped.

2. **Direct API call**: If model switching isn't available, use `bash` to call the provider's API directly:
   ```bash
   curl -s https://api.example.com/v1/chat/completions \
     -H "Authorization: Bearer $API_KEY" \
     -H "Content-Type: application/json" \
     -d '{"model":"model-name","messages":[{"role":"user","content":"task here"}]}'
   ```

3. **A2A agents**: If the user has remote A2A agents configured with different models, use `a2a_send` to dispatch work to them.

## Context Handoff

Each agent receives:
- The original request
- Previous agent outputs (as context)
- Any constraints or requirements

Format the handoff like:
```
## Context from previous phases

### Planner said:
<planner output>

### Your task:
<executor task>

### Constraints:
<user constraints>
```

## Error Handling

- If a provider/model isn't configured, tell the user which config key is missing
- If an API call fails, try the fallback model for that role
- If an agent produces empty/broken output, retry once, then report to the user
- Never silently skip a phase — always report what happened

## Examples

### Example 1: Code review across models
```
User: Review this PR with different models

Team:
  - role: reviewer-1
    provider: anthropic
    model: claude-opus-4-20250514
    task: "Review for architecture and design patterns"
  - role: reviewer-2
    provider: openrouter
    model: google/gemini-2.5-pro
    task: "Review for security vulnerabilities and edge cases"
  - role: synthesizer
    provider: openrouter
    model: anthropic/claude-sonnet-4
    task: "Combine both reviews into a single prioritized report"
```

### Example 2: Feature development
```
User: Build a rate limiter middleware

Team:
  - role: architect
    provider: anthropic
    model: claude-opus-4-20250514
    task: "Design the rate limiter: algorithm, data structures, API"
  - role: implementer
    provider: openrouter
    model: anthropic/claude-sonnet-4
    task: "Implement the design in Rust with tests"
  - role: security-auditor
    provider: openrouter
    model: google/gemini-2.5-pro
    task: "Audit for race conditions, memory safety, bypass vectors"
```

### Example 3: Documentation generation
```
User: Generate docs for this API

Team:
  - role: analyzer
    provider: openrouter
    model: google/gemini-2.5-pro
    task: "Read all source files and extract API signatures, types, examples"
  - role: writer
    provider: openrouter
    model: openai/gpt-4.1
    task: "Write clear, concise documentation from the extracted API data"
  - role: reviewer
    provider: xiaomi
    model: mimo-v2-omni
    task: "Check docs for accuracy against the source code"
```
