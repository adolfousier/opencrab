//! RSI (Recursive Self-Improvement) background engine.
//!
//! Runs as a background task after startup:
//! 1. Writes a digest of feedback_ledger stats to `~/.opencrabs/rsi/digest.md`
//! 2. Periodically analyzes feedback and applies improvements autonomously
//! 3. Emits TUI notifications when improvements are applied
//!
//! Uses the provider/model configured in `[agent].self_improvement_provider`
//! and `[agent].self_improvement_model`, falling back to the active provider.

use crate::config::Config;
use crate::db::repository::FeedbackLedgerRepository;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Interval between RSI cycles (analyze + improve).
const RSI_CYCLE_INTERVAL_SECS: u64 = 3600; // 1 hour

/// Minimum feedback entries before RSI attempts improvements.
const RSI_MIN_ENTRIES: i64 = 50;

/// Max tool iterations for the RSI agent (keep it focused).
const RSI_MAX_TOOL_ITERATIONS: usize = 10;

/// How often to run the brain-file dedup scan (in RSI cycles).
/// At 1 hour per cycle, 24 cycles = once per day.
const DEDUP_SCAN_EVERY_N_CYCLES: u64 = 24;

/// Ensure `~/.opencrabs/rsi/` and `~/.opencrabs/rsi/history/` exist.
fn ensure_rsi_dirs() -> std::io::Result<PathBuf> {
    let home = crate::config::opencrabs_home();
    let rsi_dir = home.join("rsi");
    let history_dir = rsi_dir.join("history");
    std::fs::create_dir_all(&history_dir)?;
    Ok(rsi_dir)
}

/// SHA-256 hex digest of the joined opportunity descriptions. Used to
/// detect cycle-over-cycle telemetry stability so we don't re-emit the
/// same top-N corrections / errors / tool-failure block when nothing
/// meaningful has changed.
///
/// Joining with a sentinel that can't appear inside a single description
/// (the leading `\n---\n` line marker) prevents two adjacent
/// descriptions from collapsing into the same hash as one merged one.
pub(crate) fn hash_opportunities(opps: &[String]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(opps.join("\n---\n").as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Write the startup digest to `~/.opencrabs/rsi/digest.md`.
/// Called once at boot after DB is ready.
pub async fn write_startup_digest(pool: crate::db::Pool) {
    let repo = FeedbackLedgerRepository::new(pool);
    let total = match repo.total_count().await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("RSI digest: failed to query feedback_ledger: {e}");
            return;
        }
    };

    if total == 0 {
        tracing::debug!("RSI digest: no feedback data yet, skipping");
        return;
    }

    let rsi_dir = match ensure_rsi_dirs() {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("RSI digest: failed to create rsi dir: {e}");
            return;
        }
    };

    let mut out = format!(
        "# RSI Digest\n\n**Generated:** {}\n**Total events:** {total}\n\n",
        chrono::Utc::now().format("%Y-%m-%d %H:%M UTC"),
    );

    // Event type breakdown
    if let Ok(summary) = repo.summary().await {
        out.push_str("## Event Breakdown\n\n");
        for (event_type, count) in &summary {
            let pct = (*count as f64 / total as f64) * 100.0;
            out.push_str(&format!("- **{event_type}**: {count} ({pct:.1}%)\n"));
        }
        out.push('\n');
    }

    // Tool stats with failure rates
    if let Ok(stats) = repo.stats_by_dimension("tool_").await {
        let failing: Vec<_> = stats.iter().filter(|s| s.failures > 0).collect();
        if !failing.is_empty() {
            out.push_str("## Tool Performance\n\n");
            out.push_str("| Tool | Total | OK | Fail | Rate |\n");
            out.push_str("|------|------:|---:|-----:|-----:|\n");
            for s in &failing {
                out.push_str(&format!(
                    "| {} | {} | {} | {} | {:.0}% |\n",
                    s.dimension,
                    s.total_events,
                    s.successes,
                    s.failures,
                    s.success_rate * 100.0
                ));
            }
            out.push('\n');
        }
    }

    // Recent failures
    if let Ok(entries) = repo.by_event_type("tool_failure", 10).await
        && !entries.is_empty()
    {
        out.push_str("## Recent Failures\n\n");
        for e in &entries {
            let meta = e.metadata.as_deref().unwrap_or("(no details)");
            let short: String = meta.chars().take(120).collect();
            out.push_str(&format!(
                "- `{}` — {} — {}\n",
                e.created_at.format("%Y-%m-%d %H:%M"),
                e.dimension,
                short
            ));
        }
        out.push('\n');
    }

    // User corrections
    if let Ok(corrections) = repo.by_event_type("user_correction", 10).await
        && !corrections.is_empty()
    {
        out.push_str("## User Corrections\n\n");
        for c in &corrections {
            let meta = c.metadata.as_deref().unwrap_or("(no details)");
            let short: String = meta.chars().take(120).collect();
            out.push_str(&format!(
                "- `{}` — {} — {}\n",
                c.created_at.format("%Y-%m-%d %H:%M"),
                c.dimension,
                short
            ));
        }
        out.push('\n');
    }

    // Applied improvements
    if let Ok(improvements) = repo.by_event_type("improvement_applied", 10).await
        && !improvements.is_empty()
    {
        out.push_str("## Applied Improvements\n\n");
        for imp in &improvements {
            out.push_str(&format!(
                "- `{}` — {}\n",
                imp.created_at.format("%Y-%m-%d %H:%M"),
                imp.dimension
            ));
        }
        out.push('\n');
    }

    let digest_path = rsi_dir.join("digest.md");
    match std::fs::File::create(&digest_path) {
        Ok(mut f) => {
            if let Err(e) = f.write_all(out.as_bytes()) {
                tracing::warn!("RSI digest: failed to write: {e}");
            } else {
                tracing::info!(
                    "RSI digest written to {} ({total} events)",
                    digest_path.display()
                );
            }
        }
        Err(e) => tracing::warn!("RSI digest: failed to create file: {e}"),
    }
}

/// Notification message from the RSI engine to TUI/channels.
#[derive(Debug, Clone)]
pub enum RsiNotification {
    /// RSI cycle started
    CycleStarted,
    /// Digest written at startup
    DigestWritten { total_events: i64 },
    /// Template sync completed (upstream brain files updated)
    TemplateSyncComplete { summary: String },
    /// Template sync failed
    TemplateSyncFailed { error: String },
    /// An improvement was identified and needs agent execution
    ImprovementOpportunity { description: String },
    /// Autonomous agent completed an improvement cycle
    AgentCycleComplete { summary: String },
    /// Autonomous agent failed
    AgentCycleFailed { error: String },
}

/// Format an RSI notification into its TUI display line, with secrets
/// redacted.
///
/// The `error`, `summary`, and `description` fields are free text sourced
/// from provider errors, feedback records, and tool output — any of which
/// can contain an API key, Bearer token, or credentialed URL. Without
/// redaction these surfaced on screen (2026-06-07). `redact_secrets` masks
/// key prefixes, long opaque tokens, inline "Bearer <token>", and
/// env-style secret assignments. Redaction happens here, at the single
/// formatting point, so every variant and every caller is covered.
pub(crate) fn format_rsi_notification(notification: &RsiNotification) -> String {
    let msg = match notification {
        RsiNotification::DigestWritten { total_events } => {
            format!("RSI: digest written ({total_events} events)")
        }
        RsiNotification::CycleStarted => "RSI: analyzing feedback patterns...".to_string(),
        RsiNotification::ImprovementOpportunity { description } => {
            format!("RSI: {description}")
        }
        RsiNotification::AgentCycleComplete { summary } => {
            format!("RSI: agent cycle complete — {summary}")
        }
        RsiNotification::AgentCycleFailed { error } => {
            format!("RSI: agent cycle failed — {error}")
        }
        RsiNotification::TemplateSyncComplete { summary } => {
            format!("RSI: template sync complete — {summary}")
        }
        RsiNotification::TemplateSyncFailed { error } => {
            format!("RSI: template sync failed — {error}")
        }
    };
    // Compose both redactors: redact_command catches command/URL patterns
    // (api_key= query params, --header secrets, https://user:pass@ URLs)
    // that RSI provider errors commonly carry; redact_secrets then masks
    // key prefixes, long opaque tokens, and inline Bearer values. Each is
    // a no-op on text the other already masked.
    let command_safe = crate::utils::sanitize::redact_command(&msg);
    crate::utils::sanitize::redact_secrets(&command_safe)
}

/// Build a minimal tool registry containing only the RSI tools.
/// Does the RSI session's stored pair disagree with what config selects (#805)?
///
/// The session outlives the config, and `ensure_session_provider_restored`
/// (#704) restores the SESSION's saved provider at turn start. Right for a user
/// session; wrong for this one, where config is the authority. Without this
/// check a session created months ago re-pins its original provider on every
/// cycle and `self_improvement_provider` has no effect at all.
///
/// An unset `self_improvement_model` is not a disagreement: the provider's own
/// default is intended, so the stored model is left alone rather than cleared.
pub(crate) fn rsi_pair_is_stale(
    session_provider: Option<&str>,
    session_model: Option<&str>,
    configured_provider: &str,
    configured_model: Option<&str>,
) -> bool {
    if session_provider != Some(configured_provider) {
        return true;
    }
    matches!(configured_model, Some(m) if session_model != Some(m))
}

fn build_rsi_tool_registry() -> Arc<crate::brain::tools::ToolRegistry> {
    use crate::brain::tools::ToolRegistry;
    use crate::brain::tools::feedback_analyze::FeedbackAnalyzeTool;
    use crate::brain::tools::feedback_record::FeedbackRecordTool;
    use crate::brain::tools::rsi_propose::RsiProposeTool;
    use crate::brain::tools::self_improve::SelfImproveTool;

    let registry = ToolRegistry::new();
    registry.register(Arc::new(FeedbackRecordTool));
    registry.register(Arc::new(FeedbackAnalyzeTool));
    registry.register(Arc::new(SelfImproveTool));
    // rsi_propose lets the loop file tool/command proposals to the inbox.
    // Apply path goes through rsi_proposals (user-facing), not RSI.
    registry.register(Arc::new(RsiProposeTool));
    Arc::new(registry)
}

/// The system prompt for the RSI agent.
pub(crate) const RSI_AGENT_PROMPT: &str = "\
You are the RSI (Recursive Self-Improvement) engine for OpenCrabs. \
Your job is to analyze system feedback and autonomously apply improvements to brain files.

## Analysis Steps

1. Call feedback_analyze with query='summary' to see overall system stats.
2. Call feedback_analyze with query='tool_stats' to identify tools with high failure rates.
3. Call feedback_analyze with query='failures' to see recent failure details.
4. Call feedback_analyze with query='recent' to see the latest events (including self-heal triggers).
5. For each actionable problem, call self_improve to apply a targeted fix, \
   or rsi_propose when the gap is a missing capability rather than missing guidance.
6. Be conservative: only apply improvements when you have clear evidence from the feedback data.
7. Focus on the highest-impact issues first (highest failure rate, most frequent corrections).

## Two Ways To Improve: Guidance vs Capability

- **self_improve** changes how the agent BEHAVES with what it already has: a rule, a routing \
  preference, a correction recorded in a brain file. Use it when the ability exists and only \
  direction is missing.
- **rsi_propose** asks for something the agent CANNOT currently do: a new tool, slash command, \
  or skill. Use it when no rule could fix the finding because the capability itself is absent.

Writing a rule for a missing capability does not work, and stacking such rules is the prompt \
bloat warned about below. Discarding the finding is equally wrong: it is real, and proposing \
is how it gets addressed.

Proposals are NOT installed. They go to the user's inbox for review, so a proposal is a \
suggestion with evidence attached, never a change you made.

Apply the same bar as everything else here: propose from a REPEATED pattern in the feedback \
data, never from a single occurrence. A noisy inbox is as useless as an empty one.

## Tool-Failure Triage (ask binary questions before acting)

A high failure rate is NOT sufficient reason to act. Before writing ANY rule about a tool, \
answer these yes/no questions from the feedback data and act ONLY when the answers say a \
prompt change will actually help:

1. **Enough evidence?** Skip any tool with fewer than 5 recorded calls — a 0/1 or 1/2 sample \
   is noise, not a pattern.
2. **Real defect, or recoverable/environmental?** Stale-hash 'file may have changed' retries \
   (hashline_edit), 'not connected' (channel send tools), and cancelled/timed-out prompts \
   (follow_up_question) are EXPECTED outcomes, not defects. They are already kept out of the \
   success-rate denominator — never treat them as failures or write rules about them.
3. **Misuse, or broken?** If the failures are the agent calling the tool wrong (bad params, \
   unknown action), the fix is concise USAGE guidance — never avoiding the tool.
4. **Capability, or guidance?** Prompt rules only help when the model HAS the ability and just \
   needs direction. If the failure reflects a hard limitation (the tool genuinely cannot do X), \
   a rule won't fix it and only adds noise — do NOT write one. This is the case rsi_propose \
   exists for: propose the missing capability instead of discarding the finding.

**NEVER tell the agent to avoid, ban, stop using, or 'DO NOT USE' a BUILT-IN tool.** Built-in \
tools are part of the system; banning one removes capability instead of fixing anything, and \
the self_improve guard will reject it. At most write routing guidance ('prefer X over Y for \
case Z'). Only a USER-DEFINED (tools.toml) tool may be disabled.

**Avoid prompt bloat.** Do not bump violation counters, restate an existing rule, or stack \
competing instructions across runs — accumulated lessons degrade the brain into contradictions. \
One clear rule per problem; refine in place (action='update') rather than appending near-duplicates. \
If a rule already covers the problem, leave it.

## Target File Taxonomy

Each brain file controls a different aspect of the agent. Route improvements to the RIGHT file:

- **SOUL.md** — PERSONALITY / voice: response style, tone, reasoning patterns. \
  Fix here when: phantom_tool_call events (model narrates instead of acting), gaslighting \
  preambles, verbose/repetitive responses, wrong tone. \
  NOT the hard rules / safety gates — those go in AGENTS.md (always-loaded).
- **TOOLS.md** — Tool DEFINITIONS: parameter formats, executor types, usage docs. \
  This is a reference file, NOT a dumping ground for failure logs or error notes. \
  Tool failure patterns are tracked by the feedback system (feedback_record, feedback_analyze). \
  Do NOT append error handling guidance, failure counts, or incident logs here. \
  Only edit TOOLS.md when a tool's actual definition or usage docs need updating.
- **USER.md** — How to interact with THIS USER: preferences, corrections, frustrations. \
  Fix here when: user_correction events show a repeated preference the agent keeps violating.
- **MEMORY.md** — Persistent KNOWLEDGE: facts, context, project state, integrations. \
  Fix here when: the agent repeatedly lacks context it should have retained across sessions.
- **AGENTS.md** — Workspace PROCESS + the **enforced hard rules / safety gates** (never \
  delete/push/email/post without approval). It is ALWAYS-LOADED, so any must-always-respect \
  rule a user/feedback teaches goes HERE — never in an on-demand file (MEMORY/TOOLS/CODE) where \
  it wouldn't be enforced on a cold session or after compaction. \
  Fix here when: workspace/process behavior needs adjustment, or a new hard rule is learned. \
  NOT security policy (→ SECURITY.md), NOT personality/tone (→ SOUL.md).
- **CODE.md** — Coding standards, testing, and the user's language/framework preference. \
  Fix here when: code-quality feedback recurs (wrong style, missing tests, bad patterns).
- **SECURITY.md** — Security policy: code review, network posture, data handling, credential/server access. \
  Fix here when: security-related feedback appears.
- **BOOT.md** — Startup + runtime self-maintenance: boot steps, memory-save triggers, upgrade/evolve, \
  running as a service. Fix here when: startup/persistence guidance or the memory-save triggers \
  need updating.

One kind of content per file — never duplicate a rule across files (copies drift and go stale), \
and match each file's `**Owns:**` header. SOUL/AGENTS/CODE/TOOLS/SECURITY/BOOT are generic (same for \
everyone); USER/MEMORY are user-specific.

### Custom Reference Files

Additional `.md` files may exist alongside the core brain files (the user's own custom \
notes or skill-specific docs). These are NOT core brain files. They are user-curated reference material \
loaded on demand via `load_brain_file` for inflight context. \
You may read them for context, but do NOT autonomously write to them via self_improve. \
If feedback relates to content in a custom file, suggest the change to the user instead.

## Self-Heal Event Types

These events in the feedback ledger represent behaviors the self-heal layer had to correct at runtime. \
Your job is to write improvements that PREVENT these from recurring:

- **phantom_tool_call** — Model described file changes in prose but executed zero tool calls. \
  Self-heal injected a retry prompt. Write to SOUL.md: reinforce 'execute tools, don't narrate'.
- **user_correction** — User said 'no', 'wrong', 'try again', etc. \
  Analyze the correction content to determine if it's behavioral (SOUL), tool-usage (TOOLS), or preference (USER).
- **context_compaction** — Context exceeded budget, had to be compacted. \
  If frequent, check if the agent is loading too many brain files or being too verbose (SOUL).
- **provider_error** — Provider returned an error. Usually not actionable unless the agent is \
  sending bad requests (TOOLS) or using the wrong model.
- **tool_failure** — A specific tool failed. Use feedback_record/feedback_analyze to log \
  and review patterns. Do NOT append failure notes to TOOLS.md — it's for tool definitions only.

## Workflow — MANDATORY

1. **Read first**: Before ANY modification, call self_improve with action='read' on the target file. \
   You MUST see the current content to judge whether your improvement is new, redundant, or refines something existing.
2. **Decide action**: After reading:
   - If the file has NO existing instruction covering your improvement → use action='apply' to append.
   - If the file ALREADY has an instruction that covers the same topic but needs refinement → use action='update' with the exact old_content copied from what you just read, and your improved content in 'content'.
   - If the file already covers the topic AND the feedback shows a FRESH repeat violation (new incident since the rule was written) → use action='update' to reinforce: append the new date/incident as evidence, and tighten the wording if the model keeps slipping past it. Do NOT bump inline counters — see \"Reinforcing Repeat Violations\" below. 
   Repeat violations of an existing rule are NOT a 'covered, skip' case — they signal the rule needs reinforcement.
   - If the file already says what you want to say AND there is no fresh evidence of new violations → SKIP. Do not duplicate.
3. **Never rewrite the whole file**. The 'update' action replaces ONE specific section/paragraph. \
   The 'apply' action appends. Neither should be used to rewrite the entire file. \
   Brain files contain user-written content — you must preserve it and only add/refine specific instructions.

## Reinforcing Repeat Violations

When feedback shows the same correction pattern recurring (same dimension in user_correction or
self_heal events, same root cause), update the existing rule to document the new incident:

- Find the existing rule in the brain file via action='read'.
- Use action='update' with old_content being the exact current rule text.
- **Cap at 2 evidence entries** in the rule itself. After 2 dated entries, replace
  subsequent appends with a single inline counter: `Violations: N, last: YYYY-MM-DD`
  and increment N each time. Do NOT keep appending new date/session paragraphs.
  Full incident history lives in the feedback ledger (feedback_analyze), not the
  brain file. Two evidence entries is enough to prove recurrence; more just bloat.
- Tighten the wording if the model keeps slipping past it.

**Do NOT bump inline counters** (e.g. do NOT write `Violations: 6 → 7`). The feedback ledger SQLite
database (`feedback.db` in your OpenCrabs home) is the canonical source of truth for event counts. SOUL.md
counters are decorative and go stale — they are not read by the runtime. Only the DB is queried
by feedback_analyze and the tool_loop.rs runtime.

**Do NOT append unbounded incident logs.** Each new date/session entry looks like 'new content'
to the dedup guard (issue #197) because the timestamp is unique. This causes brain-file bloat.
Use the 2-entry cap above, then the inline counter.

Skipping a repeat-violation case because 'the rule already exists' is the most common RSI
failure mode. Don't do it. The rule existing IS the reason to reinforce — but document via
evidence appends, not counter bumps.

## Proposing New Tools / Commands (rsi_propose)

You can also propose NEW dynamic tools (`tools.toml`) or NEW slash \
commands (`commands.toml`) when feedback shows the agent worked around \
a missing capability. Use `rsi_propose` for this. You do NOT install — proposals \
land in your `rsi/proposed_*.toml` inbox. The user (or the user-facing \
agent on their behalf) reviews and applies via the `rsi_proposals` tool.

Once applied, a new slash command or skill is **discoverable automatically** — the \
agent's system prompt injects a live Available Commands & Skills index every turn \
(built from `commands.toml` + `skills/`), so you do NOT need to also document it in a \
brain file for the agent to find it. Write a clear `description` — that's what the \
agent reads to decide when to run it.

When to propose a tool (kind='tool'):
- A specific bash invocation appears repeatedly across sessions (e.g. `gh issue list`, \
  `docker ps`, a curl to a private API). Wrap it as a shell tool with named params.
- The agent calls `http_request` to the same endpoint multiple times with similar \
  payloads. Wrap it as an http tool.
- Only propose tools whose execution is safe by default (read-only verbs, \
  GET requests). Set `requires_approval=true` for anything shell-based.

EFFICIENCY GATE (required for all tool proposals):
The rationale MUST explicitly state which of these applies. If none apply, do NOT \
propose the tool:
1. TOKEN SAVINGS — wrapper eliminates boilerplate (multi-step resolution, auth headers, \
   JSON construction, repeated argument patterns)
2. ERROR REDUCTION — wrapper prevents a known class of failures (quoting bugs, escaping \
   issues, parameter validation, environment setup)
3. CAPABILITY ADDITION — wrapper enables something bash cannot do alone (structured output \
   parsing, protocol handling, binary data processing)

Pure passthrough wrappers (e.g. `ssh_exec` that just wraps `bash ssh user@host 'cmd'`) \
fail this gate: same token cost, no error reduction, no new capability. Reject them.

When to propose a command (kind='command'):
- The user types `/something` repeatedly that doesn't exist (look at user_correction \
  events or recent input patterns).
- A common multi-step prompt the user reuses verbatim — a slash command saves typing.

Strict rules for rsi_propose:
- The `rationale` MUST cite the feedback evidence (event types and counts) that \
  drove the proposal. No speculation.
- One proposal per cycle is plenty. Quality over quantity.
- Never propose a destructive shell tool (`rm`, `dd`, `mv`, `>`, `|sh`, etc.) — \
  those should always go through tool_manage with explicit user approval, not \
  through RSI.
- Don't repropose: rsi_propose dedups by name, but rapid resubmission still wastes \
  the user's review time. If a proposal was already filed and not applied, the \
  user has a reason; don't insist.

## Rules

Do NOT apply improvements if the data is insufficient or ambiguous. \
Quality over quantity — one well-reasoned improvement is better than many speculative ones. \
Never duplicate an existing instruction in a brain file — you have the 'read' action to check first. \
If an improvement was already applied (check self_improve action='list'), skip it. \
Use 'update' over 'apply' when an existing instruction needs rewording, not a new one added. \
When a brain rule you write names a specific non-core tool (`telegram_send`, `analyze_image`, \
`cron_manage`, `message`, ...), pair the mention with a `tool_search` reminder — a bare tool name \
reads as directly callable and invites a schema-less guess that wastes the first call. The runtime \
preamble already states this discipline globally; do not restate the general rule, just keep \
individual tool mentions you author consistent with it.\n\nTOOL LIFECYCLE: when you find instructions \
referencing a tool the agent should use, reinforce the full cycle — `tool_search` before calling, \
fallback to `tool_search` on failure, never assume a tool is unavailable without searching. If a \
brain rule only names the tool without the search reminder, the agent will guess parameters or skip \
the tool entirely. Always include the search-then-call pattern.";

/// Run a single autonomous RSI agent cycle.
///
/// Creates a lightweight AgentService with only RSI tools, sends the improvement
/// prompt, and returns the agent's summary of what it did.
async fn run_rsi_agent_cycle(
    pool: crate::db::Pool,
    config: &Config,
    opportunities: &[String],
) -> anyhow::Result<String> {
    use crate::brain::agent::AgentService;
    use crate::services::{ServiceContext, SessionService};

    // Resolve RSI provider: prefer self_improvement_provider, fall back to user's active provider
    let active_provider = config.providers.active_provider_and_model().0;
    let provider_name = config
        .agent
        .self_improvement_provider
        .as_deref()
        .unwrap_or(&active_provider);

    // #469 part A: a dead self_improvement_provider (missing key, typo,
    // removed section) must not kill the cycle — fall back to the user's
    // active provider before the chain wrap below. Errors are loud, never
    // swallowed.
    let provider =
        match crate::brain::provider::factory::create_provider_by_name(config, provider_name).await
        {
            Ok(p) => p,
            Err(e) if provider_name != active_provider => {
                tracing::warn!(
                    "RSI: self_improvement_provider '{provider_name}' failed to create ({e:#}) — \
                 falling back to active provider '{active_provider}' (#469)"
                );
                crate::brain::provider::factory::create_provider_by_name(config, &active_provider)
                    .await?
            }
            Err(e) => return Err(e),
        };

    // Apply the [providers.fallback] chain (if any) to the RSI provider
    // — same wrapping the main session path gets via
    // `create_provider_with_warning`. Before this call, the autonomous
    // loop bypassed the chain entirely: an RSI rate limit killed the
    // cycle instead of cascading to the configured fallback.
    let provider =
        crate::brain::provider::factory::wrap_with_fallback_chain(config, provider).await?;

    // A CLI provider cannot run OpenCrabs tools: `cli_handles_tools()` makes the
    // tool loop skip local execution entirely, on the understanding that the CLI
    // runs tools internally. RSI exists ONLY to call `feedback_analyze`,
    // `self_improve` and `rsi_propose`, so on such a provider every cycle is a
    // guaranteed no-op that still pays for a full turn.
    //
    // It also fails silently, which is why this is a hard refusal rather than a
    // warning: the same flag disables phantom detection, so the model narrates
    // tool calls that never execute and nothing corrects it. Observed on a live
    // install as hourly cycles reporting "I described actions but did not
    // actually execute any tool this turn" and "Same data. Stopping." for
    // months, because nothing could ever be applied (#805).
    if provider.cli_handles_tools() {
        return Err(anyhow::anyhow!(
            "RSI cannot run on '{}': it is a CLI provider, so OpenCrabs tools \
             (feedback_analyze, self_improve, rsi_propose) never execute and every cycle \
             is a silent no-op. Set [agent] self_improvement_provider to an API provider.",
            provider.name()
        ));
    }

    let service_ctx = ServiceContext::new(pool);
    let tool_registry = build_rsi_tool_registry();
    let brain_path = crate::config::opencrabs_home();

    let agent = AgentService::new(provider, service_ctx.clone(), config)
        .await
        .with_tool_registry(tool_registry)
        .with_auto_approve_tools(true)
        .with_max_tool_iterations(RSI_MAX_TOOL_ITERATIONS)
        .with_system_brain(RSI_AGENT_PROMPT.to_string())
        .with_brain_path(brain_path);

    // Reuse a persistent RSI session — keeps context across cycles so the agent
    // knows what it already improved and doesn't repeat work.
    let session_service = SessionService::new(service_ctx);
    let mut session = match session_service
        .find_session_by_title("RSI autonomous cycle")
        .await?
    {
        Some(s) => s,
        None => {
            session_service
                .create_session_with_provider(
                    Some("RSI autonomous cycle".to_string()),
                    Some(provider_name.to_string()),
                    config.agent.self_improvement_model.clone(),
                    None,
                )
                .await?
        }
    };

    // The session outlives the config, so its stored pair must follow config
    // rather than override it (#805). `ensure_session_provider_restored` (#704)
    // restores the SESSION's saved provider at turn start, which is right for a
    // user session and wrong here: this session was created in April on
    // whatever was active then, and re-pinned that provider on every cycle
    // since, so changing `self_improvement_provider` had no effect at all.
    //
    // Re-checked every cycle, not just at creation, or a later config change
    // silently keeps the old pair, which is exactly the bug.
    {
        let configured_model = config.agent.self_improvement_model.clone();
        let pair_is_stale = rsi_pair_is_stale(
            session.provider_name.as_deref(),
            session.model.as_deref(),
            provider_name,
            configured_model.as_deref(),
        );
        if pair_is_stale {
            tracing::warn!(
                "RSI session pinned to {:?}/{:?} but config selects '{}'/{:?} — repinning to config",
                session.provider_name,
                session.model,
                provider_name,
                configured_model
            );
            let mut repinned = session.clone();
            repinned.provider_name = Some(provider_name.to_string());
            if configured_model.is_some() {
                repinned.model = configured_model;
            }
            match session_service.update_session(&repinned).await {
                Ok(()) => session = repinned,
                Err(e) => tracing::warn!("Failed to repin RSI session to the configured pair: {e}"),
            }
        }
    }

    // Build the user prompt with detected opportunities
    let mut prompt = "Run an autonomous self-improvement cycle.\n\n".to_string();
    if !opportunities.is_empty() {
        prompt.push_str("Detected opportunities:\n");
        for opp in opportunities {
            prompt.push_str(&format!("- {opp}\n"));
        }
        prompt.push('\n');
    }
    prompt.push_str(
        "Analyze the feedback data, identify the highest-impact issues, and apply improvements.",
    );

    let model = config.agent.self_improvement_model.clone();

    let response = agent
        .send_message_with_tools(session.id, prompt, model)
        .await?;

    tracing::info!(
        "RSI agent cycle complete: {} tokens used, ${:.4} cost",
        response.usage.input_tokens + response.usage.output_tokens,
        response.cost
    );

    Ok(response.content)
}

/// Spawn the background RSI engine.
///
/// - Writes startup digest immediately
/// - Every `RSI_CYCLE_INTERVAL_SECS`, checks if there are actionable patterns
/// - When opportunities are found, spawns an autonomous agent to apply improvements
/// - Emits notifications to TUI via the provided channel
pub fn spawn_rsi_engine(
    pool: crate::db::Pool,
    config: &Config,
    notification_tx: mpsc::UnboundedSender<RsiNotification>,
) {
    let pool_clone = pool.clone();
    let config_clone = config.clone();
    tokio::spawn(async move {
        // Delay to let the app fully start
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;

        // 1. Upstream template sync. No version gate (#820): whether the app
        // was upgraded says nothing about whether a template changed, and
        // gating on it left #816/#817 undeliverable for as long as the release
        // took. sync_templates decides per file by content, and one whose
        // upstream is unchanged writes nothing.
        {
            let results = crate::brain::rsi_sync::sync_templates().await;
            if results.is_empty() {
                tracing::info!("RSI template sync: no files to sync");
            } else {
                let synced = results.iter().filter(|r| r.synced).count();
                let failed = results.iter().filter(|r| r.error.is_some()).count();
                let sections: usize = results.iter().map(|r| r.sections_added).sum();
                let summary = format!(
                    "{} files synced, {} failed, {} new sections (v{})",
                    synced,
                    failed,
                    sections,
                    crate::VERSION
                );
                if failed > 0 {
                    let errors: Vec<_> = results
                        .iter()
                        .filter_map(|r| r.error.as_ref().map(|e| format!("{}: {}", r.filename, e)))
                        .collect();
                    let _ = notification_tx.send(RsiNotification::TemplateSyncFailed {
                        error: errors.join("; "),
                    });
                }
                if synced > 0 {
                    let _ = notification_tx.send(RsiNotification::TemplateSyncComplete { summary });
                }
            }
        }

        // 2. Write startup digest
        write_startup_digest(pool_clone.clone()).await;
        let repo = FeedbackLedgerRepository::new(pool_clone.clone());
        if let Ok(total) = repo.total_count().await {
            let _ = notification_tx.send(RsiNotification::DigestWritten {
                total_events: total,
            });
        }

        // 2. Periodic analysis + autonomous improvement cycle
        //
        // On startup, check how long ago the last cycle ran. If the app was
        // restarted before the interval elapsed (e.g. dev recompile every
        // ~20 min), only sleep the remaining time instead of a full hour.
        // Without this, frequent restarts prevent RSI from ever firing.
        let last_cycle_path = crate::config::opencrabs_home().join("rsi/last_cycle");
        // Hash of the previous cycle's `opportunities` Vec. When the new
        // cycle's hash matches, the RSI engine skips re-emitting the same
        // top-N corrections / errors / tool-failure descriptions to the
        // TUI and channels, and skips the autonomous agent run (the LLM
        // would just write "Converged. No improvements applied." again).
        let opportunities_hash_path =
            crate::config::opencrabs_home().join("rsi/last_opportunities_hash");
        let initial_delay = if let Ok(meta) = std::fs::metadata(&last_cycle_path) {
            let elapsed = meta
                .modified()
                .ok()
                .and_then(|t| t.elapsed().ok())
                .map(|d| d.as_secs())
                .unwrap_or(RSI_CYCLE_INTERVAL_SECS);
            if elapsed >= RSI_CYCLE_INTERVAL_SECS {
                // Overdue — run soon (30s grace for app to stabilize)
                30
            } else {
                RSI_CYCLE_INTERVAL_SECS - elapsed
            }
        } else {
            // First run ever — use full interval
            RSI_CYCLE_INTERVAL_SECS
        };
        tracing::info!(
            "RSI engine: first cycle in {}m{}s",
            initial_delay / 60,
            initial_delay % 60
        );

        let cycle_number_path = crate::config::opencrabs_home().join("rsi/cycle_number");

        let mut first_iteration = true;
        let mut last_seen_count: i64 = 0;
        // Persist cycle_number across restarts so the dedup scan
        // (every 24 cycles) actually fires. Without this, frequent
        // restarts reset the counter and dedup never triggers.
        let mut cycle_number: u64 = std::fs::read_to_string(&cycle_number_path)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        loop {
            let delay = if first_iteration {
                first_iteration = false;
                initial_delay
            } else {
                RSI_CYCLE_INTERVAL_SECS
            };
            tokio::time::sleep(std::time::Duration::from_secs(delay)).await;

            let total = match repo.total_count().await {
                Ok(t) => t,
                Err(_) => continue,
            };

            if total < RSI_MIN_ENTRIES {
                tracing::debug!(
                    "RSI cycle: only {total} entries (need {RSI_MIN_ENTRIES}), skipping"
                );
                continue;
            }

            // Skip if no new feedback since last cycle — same data = same analysis
            if total == last_seen_count {
                tracing::debug!("RSI cycle: feedback count unchanged ({total}), skipping");
                // Still stamp the file so restart timer stays accurate
                let _ = std::fs::write(&last_cycle_path, "");
                continue;
            }
            last_seen_count = total;

            let _ = notification_tx.send(RsiNotification::CycleStarted);
            tracing::info!("RSI cycle: analyzing {total} feedback entries");

            // Refresh digest file
            write_startup_digest(repo.pool().clone()).await;

            // Collect actionable opportunities
            let mut opportunities = Vec::new();

            // Tools with >20% failure rate and >5 executions over the
            // last 7 days. Without the window, a tool that broke once
            // and was fixed shows "100% failure" forever — the
            // 2026-04-25 RSI logs were full of stale alerts about
            // exa_search and wait_agent long after both bugs landed.
            let window_since = (chrono::Utc::now() - chrono::Duration::days(7))
                .format("%Y-%m-%dT%H:%M:%SZ")
                .to_string();
            // Resolve the opencrabs source repo once per cycle so we
            // can ask `git log` whether a given tool's failures already
            // have a fix commit between them and now. Returns None when
            // we can't find a checkout (installed binary launched from
            // an unrelated cwd, no OPENCRABS_SRC env var) — we then
            // skip the git-context check, falling back to the
            // window-only behaviour.
            let source_repo = crate::brain::rsi_git_history::resolve_source_repo();
            if let Ok(stats) = repo
                .stats_by_dimension_since("tool_", Some(&window_since))
                .await
            {
                // Sentinel dimensions that fire as "failures" by design
                // (self-heal detectors, regression probes). Excluding them
                // prevents noise like "phantom_intent_loop has 100% failure".
                const SENTINEL_DIMENSIONS: &[&str] = &[
                    "phantom_intent_loop",
                    "phantom_tool_call",
                    "self_improve_exact_match_fail",
                    "sticky_fallback_regression",
                    "thinking_persistence_qwen36",
                    "", // empty tool name (internal bookkeeping)
                ];
                for s in stats
                    .iter()
                    .filter(|s| s.total_events >= 5 && s.success_rate < 0.8)
                    .filter(|s| !SENTINEL_DIMENSIONS.contains(&s.dimension.as_str()))
                {
                    // Suppress the alert when the source repo has a
                    // commit since the window opened whose subject
                    // mentions this dimension (= tool name). Convention
                    // here: nearly every fix commit names the tool in
                    // its subject ("fix(provider): unwrap proxy",
                    // "fix(browser): name the actual browser"), so a
                    // grep on `dimension` against `--since=window_start`
                    // catches "we already fixed that".
                    if let Some(ref repo_path) = source_repo {
                        let commits = crate::brain::rsi_git_history::commits_matching_since(
                            repo_path,
                            &window_since,
                            &s.dimension,
                        );
                        if !commits.is_empty() {
                            tracing::info!(
                                "RSI suppress '{}': {} fix commit(s) since window open — first: {} {}",
                                s.dimension,
                                commits.len(),
                                &commits[0].sha[..7.min(commits[0].sha.len())],
                                commits[0].subject,
                            );
                            continue;
                        }
                    }
                    // Pull recent failures for this tool to give agent context
                    let mut detail = format!(
                        "Tool '{}' has {:.0}% failure rate ({} failures out of {}). \
                         Review failure patterns via feedback_analyze and record \
                         derived rules with feedback_record.",
                        s.dimension,
                        (1.0 - s.success_rate) * 100.0,
                        s.failures,
                        s.total_events
                    );
                    if let Ok(recent) = repo.by_event_type("tool_failure", 10).await {
                        let relevant: Vec<_> = recent
                            .iter()
                            .filter(|e| e.dimension == s.dimension)
                            .take(3)
                            .collect();
                        if !relevant.is_empty() {
                            detail.push_str("\n  Recent failures:");
                            for e in relevant {
                                detail.push_str(&format!(
                                    "\n  - session={}, time={}, meta={}",
                                    &e.session_id[..8.min(e.session_id.len())],
                                    e.created_at.format("%Y-%m-%d %H:%M"),
                                    e.metadata.as_deref().unwrap_or("none")
                                ));
                            }
                        }
                    }
                    tracing::info!("RSI opportunity: {}", detail);
                    opportunities.push(detail);
                }
            }

            // Repeated user corrections — include recent examples with session/model
            if let Ok(corrections) = repo.by_event_type("user_correction", 50).await
                && corrections.len() >= 3
            {
                let mut desc = format!(
                    "{} user corrections recorded. Review patterns and update brain files.",
                    corrections.len()
                );
                desc.push_str("\n  Recent corrections:");
                for e in corrections.iter().take(5) {
                    desc.push_str(&format!(
                        "\n  - session={}, model={}, time={}, text={}",
                        &e.session_id[..8.min(e.session_id.len())],
                        e.dimension,
                        e.created_at.format("%Y-%m-%d %H:%M"),
                        e.metadata.as_deref().unwrap_or("none")
                    ));
                }
                tracing::info!("RSI opportunity: {}", desc);
                opportunities.push(desc);
            }

            // Provider errors — surface model/provider info so agent knows which
            // provider is failing and can adjust brain files accordingly
            if let Ok(errors) = repo.by_event_type("provider_error", 20).await
                && errors.len() >= 3
            {
                let mut desc = format!("{} provider errors recorded.", errors.len());
                desc.push_str("\n  Recent errors:");
                for e in errors.iter().take(5) {
                    desc.push_str(&format!(
                        "\n  - session={}, provider/model={}, time={}, detail={}",
                        &e.session_id[..8.min(e.session_id.len())],
                        e.dimension,
                        e.created_at.format("%Y-%m-%d %H:%M"),
                        e.metadata.as_deref().unwrap_or("none")
                    ));
                }
                tracing::info!("RSI opportunity: {}", desc);
                opportunities.push(desc);
            }

            // Successful bash patterns — high-frequency subsystems
            // (gh, git, docker, …) flag tool-extraction candidates.
            // RSI's previous passes only walked failures, which meant
            // a workflow the agent ran 50 times successfully (e.g.
            // `gh issue comment`) never surfaced as an improvement
            // opportunity. This pass closes that gap: cmd= metadata
            // (now recorded on both success + failure events) is
            // classified by `rsi_subsystem` and grouped — subsystems
            // above PROMOTE_BASH_THRESHOLD bubble up so the RSI
            // agent can decide whether to file a tool / skill
            // proposal via rsi_propose.
            //
            // The threshold is deliberately high (~15 in a 24h
            // window) so we don't propose tools for trivial
            // one-offs. If the agent ran the same subsystem 15+
            // times in a day, it's a real pattern worth codifying.
            const PROMOTE_BASH_THRESHOLD: usize = 15;
            if let Ok(successes) = repo.by_event_type("tool_success", 2000).await {
                use std::collections::HashMap;
                let mut by_subsystem: HashMap<&'static str, Vec<&str>> = HashMap::new();
                for e in &successes {
                    if e.dimension != "bash" {
                        continue;
                    }
                    // Stay inside the analysis window so old data
                    // doesn't dominate the count.
                    if e.created_at.to_rfc3339() < window_since {
                        continue;
                    }
                    let Some(meta) = e.metadata.as_deref() else {
                        continue;
                    };
                    let Some(cmd) = crate::brain::rsi_subsystem::extract_cmd_from_meta(meta) else {
                        continue;
                    };
                    if let Some(subsystem) = crate::brain::rsi_subsystem::classify_bash_command(cmd)
                    {
                        by_subsystem.entry(subsystem).or_default().push(cmd);
                    }
                }
                // Stable order so the dedup hash below doesn't churn
                // on equivalent state.
                let mut subsystems: Vec<(&'static str, Vec<&str>)> =
                    by_subsystem.into_iter().collect();
                subsystems.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(b.0)));
                for (subsystem, cmds) in subsystems {
                    if cmds.len() < PROMOTE_BASH_THRESHOLD {
                        continue;
                    }
                    let sample: Vec<String> = cmds
                        .iter()
                        .take(5)
                        .map(|c| c.chars().take(140).collect::<String>())
                        .collect();
                    let desc = format!(
                        "Bash subsystem '{subsystem}' has {} successful invocations in the window. \
                         Promotion candidate: file a tool (rsi_propose kind=tool) for the recurring \
                         command shape, or a skill (kind=skill) for the workflow it codifies. \
                         The right shape depends on whether the calls share a parameterised invocation \
                         (→ tool) or are a multi-step sequence (→ skill). \
                         Sample invocations:\n  - {}",
                        cmds.len(),
                        sample.join("\n  - "),
                    );
                    tracing::info!("RSI opportunity: {}", desc);
                    opportunities.push(desc);
                }
            }

            // Repeated USER REQUESTS — slash-command candidates (#504).
            // The bash pass above codifies what the AGENT runs; this codifies
            // what the USER keeps typing (the /standup case). Group recent
            // user-message texts by normalized signature; a signature seen
            // COMMAND_PATTERN_THRESHOLD+ times is a promotion candidate the
            // agent files via rsi_propose kind=command. Already-applied /
            // rejected names never re-surface (#502), so this is safe to
            // suggest freely.
            {
                let cm_repo = crate::db::ChannelMessageRepository::new(repo.pool().clone());
                if let Ok(requests) = cm_repo.recent_user_requests(1000).await {
                    let candidates = crate::brain::rsi_command_patterns::command_candidates(
                        &requests,
                        crate::brain::rsi_command_patterns::COMMAND_PATTERN_THRESHOLD,
                    );
                    for c in candidates {
                        let desc = format!(
                            "User request pattern '{}' typed {} times — candidate for a slash \
                             command (rsi_propose kind=command). File a concise /<name> whose \
                             prompt captures the recurring intent. Sample phrasings:\n  - {}",
                            c.signature,
                            c.count,
                            c.samples.join("\n  - "),
                        );
                        tracing::info!("RSI opportunity: {}", desc);
                        opportunities.push(desc);
                    }
                }
            }

            // Recurring tool SEQUENCES — skill candidates (#504). Where the
            // command pass codifies what the user types, this codifies what
            // the agent DOES: an ordered tool run recurring across many
            // sessions is a workflow worth a skill. Cross-session recurrence
            // is the signal, so a within-session loop cannot inflate it.
            {
                let te_repo =
                    crate::db::repository::ToolExecutionRepository::new(repo.pool().clone());
                if let Ok(rows) = te_repo.recent_session_tool_sequences(5000).await {
                    let sessions = crate::brain::rsi_skill_sequences::group_sessions(&rows);
                    let candidates = crate::brain::rsi_skill_sequences::skill_sequence_candidates(
                        &sessions,
                        crate::brain::rsi_skill_sequences::SEQUENCE_LEN,
                        crate::brain::rsi_skill_sequences::SEQUENCE_MIN_SESSIONS,
                    );
                    for c in candidates.into_iter().take(5) {
                        let desc = format!(
                            "Tool sequence '{}' ran in {} distinct sessions — candidate for a \
                             skill (rsi_propose kind=skill). File a SKILL.md codifying the \
                             workflow so it runs as one step instead of the manual sequence.",
                            c.sequence.join(" -> "),
                            c.sessions,
                        );
                        tracing::info!("RSI opportunity: {}", desc);
                        opportunities.push(desc);
                    }
                }
            }

            // 2b. An available upgrade is the most actionable finding there
            // is, and RSI was the one component running on a schedule that
            // never mentioned it (#821). Proposed, never applied: replacing
            // the running binary is the user's call.
            //
            // Recorded once. RSI runs hourly, and a proposal per hour for the
            // same version trains the user to ignore the inbox. check_for_update
            // already returns None when no asset exists for this platform or
            // the network failed, so neither produces a finding.
            if let Some(latest) = crate::brain::tools::evolve::check_for_update().await {
                let marker = crate::config::opencrabs_home().join("rsi/last_proposed_version");
                let already = std::fs::read_to_string(&marker)
                    .ok()
                    .map(|s| s.trim().to_string());
                if already.as_deref() != Some(latest.as_str()) {
                    if let Err(e) = std::fs::write(&marker, &latest) {
                        tracing::warn!("RSI: failed to record proposed version: {e}");
                    }
                    tracing::info!("RSI: upgrade available, {} -> {latest}", crate::VERSION);
                    opportunities.push(format!(
                        "OpenCrabs {latest} is available (running {}). This is a capability \
                         gap, not a guidance one: propose it with rsi_propose so the user can \
                         review and run /evolve. Do NOT upgrade autonomously — replacing the \
                         running binary is the user's decision.",
                        crate::VERSION
                    ));
                }
            }

            // 3. Dedup: hash the assembled opportunity descriptions and
            // compare against the previous cycle's hash. When identical,
            // the autonomous agent would have nothing new to act on — its
            // own summary on those cycles was literally "Converged. No
            // improvements applied." (seen in the 2026-05-18 transcript
            // where #426 just re-printed the top-5 corrections / errors
            // from #425). Skip emission of every `ImprovementOpportunity`
            // notification AND the agent run, keeping only a compact
            // `AgentCycleComplete` so the user sees the cycle happened.
            //
            // The hash covers the full opportunity-description bodies
            // (including the per-event session/timestamp lines), so any
            // change — new entry, reordered top-5, even a single recent
            // event that shifts the slice — counts as new and re-enables
            // the full path. `tracing::info!` logs above stay regardless.
            let current_hash = hash_opportunities(&opportunities);
            let previous_hash = std::fs::read_to_string(&opportunities_hash_path)
                .ok()
                .map(|s| s.trim().to_string());
            let is_duplicate = previous_hash.as_deref() == Some(current_hash.as_str());
            let _ = std::fs::write(&opportunities_hash_path, &current_hash);

            if is_duplicate {
                if !opportunities.is_empty() {
                    tracing::info!(
                        "RSI cycle: {} opportunity/opportunities identical to previous cycle \
                         (hash={}) — skipping emission and agent run",
                        opportunities.len(),
                        &current_hash[..12.min(current_hash.len())]
                    );
                    let _ = notification_tx.send(RsiNotification::AgentCycleComplete {
                        summary: format!(
                            "Converged — {} opportunity/opportunities identical to previous cycle; \
                             no agent run.",
                            opportunities.len()
                        ),
                    });
                }
                // empty + duplicate = baseline match, stay silent
            } else {
                // Surface every opportunity to the TUI / channels, then
                // spawn the autonomous improvement agent.
                for opp in &opportunities {
                    let _ = notification_tx.send(RsiNotification::ImprovementOpportunity {
                        description: opp.clone(),
                    });
                }
                if !opportunities.is_empty() {
                    tracing::info!(
                        "RSI cycle: {} opportunities found, spawning autonomous agent",
                        opportunities.len()
                    );
                    match run_rsi_agent_cycle(repo.pool().clone(), &config_clone, &opportunities)
                        .await
                    {
                        Ok(summary) => {
                            let short: String = summary.chars().take(200).collect();
                            tracing::info!("RSI agent completed: {short}");
                            let _ = notification_tx
                                .send(RsiNotification::AgentCycleComplete { summary });
                        }
                        Err(e) => {
                            tracing::warn!("RSI agent cycle failed: {e}");
                            let _ = notification_tx.send(RsiNotification::AgentCycleFailed {
                                error: e.to_string(),
                            });
                        }
                    }
                }
            }

            // Periodic brain-file dedup scan — runs every N cycles
            // (default: once per day at 24 x 1h cycles). Files proposals
            // into Mission Control for user review. Does NOT auto-apply.
            cycle_number += 1;
            let _ = std::fs::write(&cycle_number_path, cycle_number.to_string());
            if cycle_number.is_multiple_of(DEDUP_SCAN_EVERY_N_CYCLES) {
                let brain_path = crate::config::opencrabs_home();
                let store = crate::brain::rsi_proposals::ProposalsStore::new();
                // Housekeeping (#606): drop any pending proposal whose name is
                // already applied/rejected so the store doesn't carry stale
                // legacy entries the inbox filter only hides at read time.
                store.prune_handled();
                let filed = crate::brain::dedup_scan::file_dedup_proposals(&brain_path, &store);
                if filed > 0 {
                    tracing::info!("RSI dedup scan: filed {filed} brain-file dedup proposal(s)");
                    let _ = notification_tx.send(RsiNotification::AgentCycleComplete {
                        summary: format!("Brain dedup scan: {filed} duplicate(s) found, filed for review in Mission Control."),
                    });
                } else {
                    tracing::debug!("RSI dedup scan: no duplicates found");
                }
            }

            // Stamp last_cycle so restarts resume from here, not from scratch
            let _ = std::fs::write(&last_cycle_path, "");
        }
    });
}
