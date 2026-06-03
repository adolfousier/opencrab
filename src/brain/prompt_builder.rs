//! Brain Loader & Prompt Builder
//!
//! Reads workspace markdown files and assembles the system brain dynamically
//! each turn, so edits to brain files take effect immediately.

use crate::db::repository::feedback_ledger::FeedbackLedgerRepository;
use std::path::PathBuf;

/// Core brain files — always injected (personality + user context).
///
/// Kept lean (~8 KB) so always-injecting is cheap. TOOLS.md and CODE.md
/// moved to contextual (on-demand via `load_brain_file`) to avoid ~44k
/// first-request bloat.
const CORE_BRAIN_FILES: &[(&str, &str)] =
    &[("SOUL.md", "personality"), ("USER.md", "user profile")];

/// Contextual brain files — loaded on demand via the `load_brain_file` tool.
/// IDENTITY.md lives here — only needed for cron jobs and social media replies.
/// TOOLS.md and CODE.md moved here (2026-05) to slim core prompt.
pub(crate) const CONTEXTUAL_BRAIN_FILES: &[(&str, &str)] = &[
    ("IDENTITY.md", "identity — social/cron replies only"),
    ("AGENTS.md", "workspace rules"),
    ("CODE.md", "coding standards"),
    ("TOOLS.md", "tool notes & config"),
    ("SECURITY.md", "security policies"),
    ("MEMORY.md", "long-term memory"),
    ("BOOT.md", "startup config"),
    ("BOOTSTRAP.md", "bootstrap config"),
    ("HEARTBEAT.md", "heartbeat config"),
];

/// All brain files in assembly order — kept for `build_system_brain` (full mode).
/// IDENTITY.md excluded — only loaded on-demand for cron/social agent sessions.
/// TOOLS.md and CODE.md excluded from full mode too — they're contextual now.
const BRAIN_FILES: &[(&str, &str)] = &[
    ("SOUL.md", "personality"),
    ("USER.md", "user"),
    ("AGENTS.md", "agents"),
    ("SECURITY.md", "security"),
    ("MEMORY.md", "memory"),
    ("BOOT.md", "boot"),
    ("BOOTSTRAP.md", "bootstrap"),
    ("HEARTBEAT.md", "heartbeat"),
];

/// Brain preamble — always present regardless of workspace contents.
pub(crate) const BRAIN_PREAMBLE: &str = r#"You are OpenCrabs, an AI orchestration agent with powerful tools to help with software development tasks.

IMPORTANT: You have access to tools for file operations and code exploration. USE THEM PROACTIVELY!

TOOL CALL PROTOCOL — CRITICAL:
- Always call tools directly — never write code yourself, never describe what you plan to do. Just call the tool immediately.
- Do NOT output markdown code blocks (```bash, ```sh, ```python, etc.) — invoke the `bash` / `python` tool instead. Code blocks are TEXT, the system will NOT execute them.
- WRONG: writing ```bash\ngit status\n``` or "Let me run `git log`" — nothing runs.
- RIGHT: emit a tool_call for `bash` with {"command": "git status"} via the structured tool-call API.
- NEVER claim to have run a command, read a file, or fetched a URL when you haven't actually invoked the corresponding tool. If you need work done, call the tool. If you can't, say so.
- Thinking/reasoning is fine, but the final action MUST be either a tool_call or a direct answer — not a code block pretending to be one, not a narration of what you'd do.
- NEVER emit IDE-style inline edit formats. These look like agent tool calls but are NOT — they were trained into you by Cursor / Aider / Cline / continue.dev datasets and don't work here. Specifically forbidden patterns:
    ```lang|CODE_EDIT_BLOCK|/abs/path/file.ext      ← Cursor-style
    ```search_and_replace
    <<<<<<< SEARCH ... ======= ... >>>>>>> REPLACE   ← Aider conflict-marker style
    ```diff with file headers                       ← unified-diff dumps
  To edit a file: call the `edit_file` tool (or `write_file` for new files) with the structured tool-call API. If the file is large, read it first via `read_file`, then call `edit_file` with the precise `old_text` / `new_text`. The system will REJECT any inline-edit format and the change will NOT apply — you will have just leaked the file contents to the channel.

CRITICAL RULE: After calling tools and getting results, you MUST provide a final text response to the user.
DO NOT keep calling tools in a loop. Call the necessary tools, get results, then respond with text.

When asked to analyze or explore a codebase:
1. Use 'ls' tool with recursive=true to list all directories and files
2. Use 'glob' tool with patterns like "**/*.rs", "**/*.toml", "**/*.md" to find files
3. Use 'grep' tool to search for patterns, functions, or keywords in code
4. Use 'read_file' tool to read specific files you've identified
5. Use 'bash' tool for git operations like: git log, git diff, git branch

When asked to make changes:
1. Use 'read_file' first to understand the current code
2. Use 'edit_file' to modify existing files
3. Use 'write_file' to create new files
4. Use 'bash' to run tests or build commands

Available tools and their REQUIRED parameters (use exact parameter names):
- ls: List directory contents. Params: path (string), recursive (bool)
- glob: Find files matching patterns. Params: pattern (string, REQUIRED — e.g. "**/*.rs")
- grep: Search for text in files. Params: pattern (string, REQUIRED — the search text), path (string), regex (bool), case_insensitive (bool), file_pattern (string), limit (int), context (int)
- read_file: Read file contents. Params: path (string, REQUIRED)
- edit_file: Modify existing files. Params: path (string, REQUIRED), operation (string, REQUIRED)
- write_file: Create new files. Params: path (string, REQUIRED), content (string, REQUIRED)
- bash: Run shell commands. Params: command (string, REQUIRED)
- execute_code: Test code snippets. Params: language (string, REQUIRED), code (string, REQUIRED)
- web_search: Search the internet. Params: query (string, REQUIRED)
- http_request: Call external APIs. Params: method (string, REQUIRED), url (string, REQUIRED)
- task_manager: Track multi-step work. Params: operation (string, REQUIRED)
- session_context: Remember important facts. Params: operation (string, REQUIRED)
- session_search: Search across sessions. Params: operation (string, REQUIRED — "search" or "list"), query (string), n (int)
- plan: Create structured plans. Params: operation (string, REQUIRED)

CRITICAL: PLAN TOOL USAGE
When a user says "create a plan", "make a plan", or describes a complex multi-step task, you MUST use the plan tool immediately.
DO NOT write a text description of a plan. DO NOT explain what should be done. CALL THE TOOL.

Mandatory steps for plan creation:
1. IMMEDIATELY call plan tool with operation='create' to create a new plan
2. Call plan tool with operation='add_task' for each task (call multiple times)
   - IMPORTANT: The 'description' field MUST contain detailed implementation steps
   - Include: specific files to create/modify, functions to implement, commands to run
   - Format: Use numbered steps or bullet points for clarity
   - Be concrete: "Create Login.jsx component with email/password form fields and validation"
     NOT vague: "Create login component"
3. Call plan tool with operation='finalize' — this auto-approves the plan immediately
4. Begin executing tasks in order right away using start_task/complete_task — no waiting

NEVER generate text plans. ALWAYS use the plan tool for planning requests.

ALWAYS explore first before answering questions about a codebase. Don't guess - use the tools!

SELF-AWARENESS — CHECK WHAT YOU ALREADY HAVE BEFORE BUILDING NEW:
Before proposing to implement a feature from scratch (STT, TTS, browser automation, messaging channels, token compression, PDF rendering, etc.):
1. Check your tool list in this request — is there already a tool for this? Use it instead of bash+pip+third-party libraries.
2. Check the "Built-in features compiled into this binary" line in Runtime Info below — is the capability already baked into the OpenCrabs binary you're running? If yes, USE it; don't re-implement it.
3. Check the relevant brain file (TOOLS.md for tool usage, AGENTS.md for project conventions) before deciding the right surface.
Skipping these checks wastes the user's time, ships duplicate code, and makes the agent look unaware of its own runtime.

WEB / GITHUB / BROWSER ROUTING — pick the right surface, not the heaviest one:
- Web research, docs, "what's the latest X", "find me info about Y": use `exa_search` (if available) → `brave_search` (if available) → `web_search`. Never reach for `browser_navigate` to read pages.
- Anything on GitHub (issues, PRs, releases, comments, file contents, commits, checks, code search, workflow runs): use the `gh` CLI via `bash`. It is preinstalled, authenticated, returns structured JSON (`--json`, `--jq`), and is far cheaper than navigating github.com in a browser.
- `browser_navigate` is for: (a) the user explicitly asking you to open / interact with a page, (b) tasks that require clicking / typing / submitting / scrolling / running JS against live DOM, (c) genuine last resort after every search route has been tried and failed. It is slow, token-heavy, and steals window focus in headed mode — never the default.

FINISHING A TURN — always acknowledge clearly, never disappear silently:
Every turn that runs tool calls MUST end with a real text acknowledgement. Empty completions (`finish_reason: stop` with no content) look identical to silent crashes from the user's side — never do that. The shape of the acknowledgement depends on the task, but it is ALWAYS present:

(1) SIDE-EFFECT tasks — "commit X", "push", "edit file Y", "send a message", "deploy", "close issue N", "create PR", "tag the release":
The tool call did the work; your acknowledgement confirms WHAT actually happened with the specifics — sha of the commit, name of the file you edited, issue you closed, the count or identifier the user can reference later. One or two sentences with the real values. Examples of the right shape: "Committed as 7256f666 — 11 files changed, +363/-23." / "Edited tool_loop.rs:490, added the display_text_override fallback." / "Closed issue #138 with a comment summarising the fix."
- DO produce the acknowledgement. The user wants the confirmation; do NOT omit it. An empty close is the worst possible outcome — it looks like a silent failure to the user.
- Do NOT pad with restatements. One real sentence with the specifics is enough; ten paragraphs in different wordings is not.
- Do NOT re-narrate the tool output as if the user can't see it. The TUI / channel already showed the tool result.
- Do NOT run "verification" tool calls (re-grep the file you just edited, re-`gh pr view` the PR you just commented on, re-`git log` the commit you just made) to prove the work landed. The tool result already proved it.
- If your response starts with "I have successfully…" / "The task is complete…" / "All actions are now aligned…" / "The process has concluded…", drop the corporate boilerplate and just state what you did with the actual values. That IS the acknowledgement.

(2) DATA-FETCH / ANALYSIS tasks — "audit X", "review Y", "compare A and B", "explain Z", "summarise the PR", "check the logs", "describe the schema", "what does this code do":
The tool calls fetched data. You still owe the user a real text answer that uses that data. The fetched JSON / file contents / log lines are the INPUT to your answer, NOT the answer itself. Examples of correct closes: a one-paragraph audit summary citing the fields you found, a comparison table of A vs B, a 3-bullet review with line references, a plain-language explanation of what the code does. End once the analysis is written, not when the fetch returns.
- "Done." after `gh pr view` is WRONG when the user asked you to audit the PR — they wanted the audit.
- "Fetched." / "Got it." / "Loaded." are NOT analysis answers. They tell the user nothing they didn't already know from the tool indicator in the TUI.
- The cue is the verb in the user's request: audit / review / compare / explain / summarise / summarize / check / describe / analyse / analyze / what does / how does / why does / find — these all expect an analytical text response.

The single rule both shapes share: never end with empty content. If you've decided you have nothing to add beyond what the tool already showed, the right minimum is still one concrete sentence naming WHAT you did with the specifics — never zero text, never a bare "Done." with no context. Side-effect tasks get a short factual confirmation. Analysis tasks get the actual analysis.

RECURSIVE SELF-IMPROVEMENT:
You have three tools for improving yourself over time:
- feedback_analyze: Query your performance history (tool success rates, failure patterns, recent events). Call with query='summary' or query='tool_stats' or query='failures'.
- feedback_record: Manually log observations — user corrections, patterns you notice, strategies that work well.
- self_improve: Propose or apply changes to your brain files (SOUL.md, TOOLS.md, etc.). Runs autonomously — no human approval needed. Changes are logged to ~/.opencrabs/rsi/improvements.md and archived in ~/.opencrabs/rsi/history/.

Your tool executions are automatically tracked. When you notice recurring failures, user frustration, or repeated corrections:
1. Call feedback_analyze with query='failures' to understand what's going wrong
2. Call feedback_record to log the pattern you observed
3. Call self_improve with action='apply' to apply a concrete improvement — brain file is edited, improvement is logged to rsi/improvements.md, and a daily archive entry is created

Do NOT call these tools every turn. Use them when you notice a pattern across multiple interactions, or when a user explicitly corrects you in a way that could apply to future conversations. Report significant improvements to the TUI or connected channels so the user knows what changed."#;

/// Loads brain workspace files and assembles the system brain.
pub struct BrainLoader {
    workspace_path: PathBuf,
}

impl BrainLoader {
    /// Create a new BrainLoader with the given workspace path.
    pub fn new(workspace_path: PathBuf) -> Self {
        Self { workspace_path }
    }

    /// Resolve the brain path: `~/.opencrabs/`
    ///
    /// Brain files (SOUL.md, IDENTITY.md, etc.) live at the root of the
    /// OpenCrabs home directory for simplicity.
    pub fn resolve_path() -> PathBuf {
        crate::config::opencrabs_home()
    }

    /// Read a single markdown file from the workspace. Returns `None` if missing.
    pub fn load_file(&self, name: &str) -> Option<String> {
        let path = self.workspace_path.join(name);
        std::fs::read_to_string(&path).ok()
    }

    /// Build the full system brain from workspace files + brain preamble.
    ///
    /// Assembly order:
    /// 1. Brain preamble (hardcoded, always present)
    /// 2. SOUL.md — personality, tone, hard rules
    /// 3. IDENTITY.md — agent name, vibe, emoji
    /// 4. USER.md — who the human is
    /// 5. AGENTS.md — workspace rules, memory system, safety
    /// 6. TOOLS.md — environment-specific notes
    /// 7. MEMORY.md — long-term context
    /// 8. Runtime info — model, provider, working directory, OS, timestamp
    /// 9. Slash commands list (provided externally)
    pub fn build_system_brain(
        &self,
        runtime_info: Option<&RuntimeInfo>,
        slash_commands_section: Option<&str>,
    ) -> String {
        let mut prompt = String::with_capacity(8192);

        // 1. Brain preamble — always present
        prompt.push_str(BRAIN_PREAMBLE);
        prompt.push_str("\n\n");

        // 2-7. Brain workspace files (skip missing ones silently)
        for (filename, label) in BRAIN_FILES {
            if let Some(content) = self.load_file(filename) {
                let trimmed = content.trim();
                if !trimmed.is_empty() {
                    prompt.push_str(&format!(
                        "--- {} ({}) ---\n{}\n\n",
                        filename, label, trimmed
                    ));
                }
            }
        }

        // 8. Runtime info
        if let Some(info) = runtime_info {
            prompt.push_str("--- Runtime Info ---\n");
            if let Some(ref model) = info.model {
                prompt.push_str(&format!("Model: {}\n", model));
            }
            if let Some(ref provider) = info.provider {
                prompt.push_str(&format!("Provider: {}\n", provider));
            }
            if let Some(ref wd) = info.working_directory {
                prompt.push_str(&format!("Working directory: {}\n", wd));
                push_home_anchor_and_expansion_rule(&mut prompt);
            }
            prompt.push_str(&format!("OS: {}\n", std::env::consts::OS));
            prompt.push_str(&format!(
                "Timestamp: {}\n",
                chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
            ));
            prompt.push('\n');
        }

        // 9. Slash commands list
        if let Some(commands_section) = slash_commands_section
            && !commands_section.is_empty()
        {
            prompt.push_str("--- Available Slash Commands ---\n");
            prompt.push_str(commands_section);
            prompt.push_str("\n\n");
        }

        // 10. Available skills
        push_skills_section(&mut prompt);

        prompt
    }

    /// Build a lean "core" system brain: only SOUL.md + IDENTITY.md are injected.
    ///
    /// All other brain files (USER.md, MEMORY.md, AGENTS.md, etc.) are listed in a
    /// "Available Context Files" index section so the agent knows they exist and can
    /// load them on demand via the `load_brain_file` tool — only when actually needed.
    ///
    /// This eliminates 10–20k token overhead from requests that don't need user profile,
    /// long-term memory, or policy files.
    pub fn build_core_brain(
        &self,
        runtime_info: Option<&RuntimeInfo>,
        slash_commands_section: Option<&str>,
    ) -> String {
        let mut prompt = String::with_capacity(4096);

        // 1. Brain preamble — always present
        prompt.push_str(BRAIN_PREAMBLE);
        prompt.push_str("\n\n");

        // 2. Core files only (SOUL.md + IDENTITY.md)
        for (filename, label) in CORE_BRAIN_FILES {
            if let Some(content) = self.load_file(filename) {
                let trimmed = content.trim();
                if !trimmed.is_empty() {
                    prompt.push_str(&format!(
                        "--- {} ({}) ---\n{}\n\n",
                        filename, label, trimmed
                    ));
                }
            }
        }

        // 3. Memory index — list contextual files that exist on disk
        let available: Vec<(&str, &str)> = CONTEXTUAL_BRAIN_FILES
            .iter()
            .filter(|(name, _)| self.workspace_path.join(name).exists())
            .copied()
            .collect();

        // Discover user-created .md files not in the hardcoded list so the
        // agent knows the full brain layout (AGENTVERSE.md, VOICE.md, etc.)
        let known: std::collections::HashSet<String> = CORE_BRAIN_FILES
            .iter()
            .chain(CONTEXTUAL_BRAIN_FILES.iter())
            .map(|(n, _)| n.to_lowercase())
            .collect();
        let mut extras: Vec<String> = std::fs::read_dir(&self.workspace_path)
            .ok()
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter_map(|e| {
                        let name = e.file_name().to_string_lossy().to_string();
                        (name.ends_with(".md") && !known.contains(&name.to_lowercase()))
                            .then_some(name)
                    })
                    .collect()
            })
            .unwrap_or_default();
        extras.sort();

        if !available.is_empty() || !extras.is_empty() {
            // Anchor the brain dir path so the agent doesn't have to grep for it.
            // Render as ~/... (collapse_home) to keep the prompt cache-stable
            // across machines and avoid leaking the username.
            let brain_dir = crate::brain::tools::error::collapse_home(&self.workspace_path);
            prompt.push_str(&format!(
                "--- Available Context Files (in {}/) ---\n",
                brain_dir
            ));
            prompt.push_str(&format!(
                "Brain directory: {}/  (all files below live here)\n\
                 Load on demand with the `load_brain_file` tool when relevant — \
                 do NOT load unless the request actually needs that context. \
                 Use `write_opencrabs_file` to update or edit a brain file.\n\n",
                brain_dir
            ));
            for (name, desc) in &available {
                prompt.push_str(&format!("- **{}**: {}\n", name, desc));
            }
            for name in &extras {
                prompt.push_str(&format!("- **{}**: (user-created)\n", name));
            }
            // Guidance text: only mention files that actually exist on disk
            let has = |name: &str| available.iter().any(|(n, _)| *n == name);
            prompt.push_str("\nLoad proactively when:\n");
            if has("USER.md") {
                prompt.push_str("- User asks personal questions or preferences → load USER.md\n");
            }
            if has("MEMORY.md") {
                prompt.push_str(
                    "- Starting a project session or recalling past work → load MEMORY.md\n",
                );
            }
            if has("AGENTS.md") || has("SECURITY.md") || has("CODE.md") {
                let files: Vec<&str> = ["AGENTS.md", "SECURITY.md", "CODE.md"]
                    .iter()
                    .copied()
                    .filter(|n| has(n))
                    .collect();
                prompt.push_str(&format!(
                    "- Policy / rule / safety / coding standards check → load {}\n",
                    files.join(", ")
                ));
            }
            if has("TOOLS.md") {
                prompt
                    .push_str("- Working with environment-specific tool configs → load TOOLS.md\n");
            }
            prompt.push('\n');

            // Memory persistence hint — tell the agent to proactively write learnings
            if has("MEMORY.md") {
                prompt.push_str(
                    "Write proactively to MEMORY.md (via `write_opencrabs_file`) when:\n\
                     - You discover a fact, pattern, or context that would be valuable across sessions\n\
                     - The user corrects you on something non-obvious that isn't already in MEMORY.md\n\
                     - You learn project-specific knowledge (integrations, team structure, workflows)\n\
                     - A self-heal event fires (phantom tool call, gaslighting strip) — record what \
                     triggered it and the correct behavior so you avoid it next time\n\
                     Do NOT write ephemeral task details or anything derivable from code/git. \
                     Load MEMORY.md first to avoid duplicates before writing.\n\n",
                );
            }
        }

        // 4. Runtime info
        if let Some(info) = runtime_info {
            prompt.push_str("--- Runtime Info ---\n");
            if let Some(ref model) = info.model {
                prompt.push_str(&format!("Model: {}\n", model));
            }
            if let Some(ref provider) = info.provider {
                prompt.push_str(&format!("Provider: {}\n", provider));
            }
            if let Some(ref wd) = info.working_directory {
                prompt.push_str(&format!("Working directory: {}\n", wd));
                push_home_anchor_and_expansion_rule(&mut prompt);
            }
            prompt.push_str(&format!("OS: {}\n", std::env::consts::OS));
            prompt.push_str(&format!(
                "Timestamp: {}\n",
                chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
            ));
            push_known_paths(&mut prompt);
            push_compiled_features(&mut prompt);
            prompt.push('\n');
        }

        // 5. Slash commands list
        if let Some(commands_section) = slash_commands_section
            && !commands_section.is_empty()
        {
            prompt.push_str("--- Available Slash Commands ---\n");
            prompt.push_str(commands_section);
            prompt.push_str("\n\n");
        }

        // 6. Available skills
        push_skills_section(&mut prompt);

        prompt
    }
}

/// Build a compact performance digest from the feedback ledger.
///
/// Returns `None` if there's no data (new user) or if the DB query fails.
/// The digest is short — under 500 chars — to avoid bloating the system prompt.
pub async fn build_feedback_digest(pool: crate::db::Pool) -> Option<String> {
    let repo = FeedbackLedgerRepository::new(pool);
    let total = repo.total_count().await.ok()?;
    if total < 10 {
        return None; // Not enough data to be useful
    }

    let mut out = String::from("--- Performance History ---\n");
    out.push_str(&format!("Total tool executions recorded: {total}\n"));

    // Tool stats — show tools with >10% failure rate
    if let Ok(stats) = repo.stats_by_dimension("tool_").await {
        let mut header_written = false;
        for s in stats
            .iter()
            .filter(|s| s.failures > 0 && s.success_rate < 0.9)
            .take(5)
        {
            if !header_written {
                out.push_str("Tools with notable failure rates:\n");
                header_written = true;
            }
            out.push_str(&format!(
                "  {} — {:.0}% success ({} ok, {} fail)\n",
                s.dimension,
                s.success_rate * 100.0,
                s.successes,
                s.failures
            ));
        }
    }

    // Recent failures
    if let Ok(entries) = repo.by_event_type("tool_failure", 5).await
        && !entries.is_empty()
    {
        out.push_str("Recent failures:\n");
        for e in &entries {
            let meta = e.metadata.as_deref().unwrap_or("(no details)");
            let short: String = meta.chars().take(80).collect();
            out.push_str(&format!("  {} — {}\n", e.dimension, short));
        }
    }

    // User corrections count
    if let Ok(corrections) = repo.by_event_type("user_correction", 50).await
        && !corrections.is_empty()
    {
        out.push_str(&format!(
            "User corrections recorded: {}\n",
            corrections.len()
        ));
    }

    out.push_str(
        "Use feedback_analyze for deeper analysis. \
         If you see patterns, use self_improve to apply fixes autonomously.\n\n",
    );
    Some(out)
}

/// Runtime information injected into the system brain.
#[derive(Debug, Clone, Default)]
pub struct RuntimeInfo {
    pub model: Option<String>,
    pub provider: Option<String>,
    /// Pre-collapsed via `tools::error::collapse_home` so `$HOME` is
    /// rendered as `~/...` — saves tokens AND keeps the username out
    /// of every prompt's cache key. Callers MUST call `collapse_home`
    /// before stuffing a real path here.
    pub working_directory: Option<String>,
}

/// Append the home-anchor + tilde-expansion rule directly under the
/// `Working directory:` line.
///
/// The 2026-04-26 regression: collapsing `$HOME → ~` in the prompt
/// also stripped the literal username (e.g. `adolfousierstudio`) the
/// model used to parrot back when constructing absolute paths. With
/// nothing to copy from, the model started inventing one — typically
/// the user's first name from git config (`/Users/adolfo/...`),
/// breaking every shell command that needed an absolute path.
///
/// The fix is two short lines:
///
/// 1. Anchor `~` to the literal home so the model has ground truth if
///    it ever needs to expand it (defense in depth).
/// 2. Tell the model not to expand it itself — the shell handles `~`,
///    so passing `~/foo` to bash always works.
fn push_home_anchor_and_expansion_rule(prompt: &mut String) {
    if let Some(home) = dirs::home_dir().and_then(|p| p.to_str().map(String::from)) {
        prompt.push_str(&format!(
            "Home: {} (the '~' in paths above expands to this)\n",
            home
        ));
    }
    prompt.push_str(
        "Path expansion: when invoking shell tools (bash, etc.), pass `~/...` paths verbatim — \
         the shell expands `~` for you. Do NOT substitute `/Users/<name>/...` yourself; if you \
         need an absolute form, copy the `Home:` line above exactly.\n",
    );
}

/// List of OpenCrabs features compiled into this binary. Built at
/// runtime from `cfg!(feature = "...")` checks against every feature
/// declared in `Cargo.toml::[features]`. Used to teach the agent
/// what it already has — without this, newly-onboarded users get
/// told "let me implement local STT from scratch" when local-stt is
/// already a default feature with a working backend.
///
/// If you add a new feature to `Cargo.toml`, add it here too — the
/// `prompt_compiled_features_test::all_cargo_features_are_listed`
/// sentinel will fail otherwise.
pub(crate) fn compiled_features() -> Vec<&'static str> {
    let mut out = Vec::new();
    if cfg!(feature = "telegram") {
        out.push("telegram");
    }
    if cfg!(feature = "whatsapp") {
        out.push("whatsapp");
    }
    if cfg!(feature = "discord") {
        out.push("discord");
    }
    if cfg!(feature = "slack") {
        out.push("slack");
    }
    if cfg!(feature = "trello") {
        out.push("trello");
    }
    if cfg!(feature = "local-stt") {
        out.push("local-stt");
    }
    if cfg!(feature = "local-tts") {
        out.push("local-tts");
    }
    if cfg!(feature = "browser") {
        out.push("browser");
    }
    if cfg!(feature = "rtk") {
        out.push("rtk");
    }
    if cfg!(feature = "pdfium") {
        out.push("pdfium");
    }
    if cfg!(feature = "profiling") {
        out.push("profiling");
    }
    out
}

/// Append the "Built-in features" line that surfaces what's compiled
/// into this binary so the agent reaches for existing capabilities
/// instead of writing new ones from scratch (issue: new user asked
/// for "local STT/TTS implementation" and the agent started coding
/// when both are default features with working backends).
pub(crate) fn push_compiled_features(prompt: &mut String) {
    let features = compiled_features();
    if features.is_empty() {
        return;
    }
    prompt.push_str(&format!(
        "Built-in features compiled into this binary: {}\n\
         Before implementing any of these capabilities from scratch, USE the built-in. \
         If the user asks for a feature listed here, it already works — don't re-build it. \
         If they ask for a Cargo feature NOT in this list (e.g. `pdfium`), tell them to \
         rebuild with `--features <name>` instead of writing fresh code.\n",
        features.join(", ")
    ));
}

/// Append an "Available Skills" section listing every skill's name +
/// description so the LLM can recommend or auto-dispatch them.
///
/// This closes issue #151: skills had a `description` field documented
/// as "LLM auto-invoking" but it was never injected into the prompt.
/// Cheap fix: just list them here so the model knows what's available.
fn push_skills_section(prompt: &mut String) {
    let skills = crate::brain::skills::load_all_skills();
    if skills.is_empty() {
        return;
    }
    prompt.push_str("--- Available Skills ---\n");
    prompt.push_str(
        "Use the `slash_command` tool with `/skill-name` to invoke these workflow templates.\n",
    );
    for skill in &skills {
        prompt.push_str(&format!(
            "- **{}**: {}\n",
            skill.slash_name, skill.description
        ));
    }
    prompt.push('\n');
}

/// Append a "Known paths" section to the runtime info so when the
/// user says "check the logs" the agent knows EXACTLY where to look
/// instead of grepping random places in the working directory.
///
/// All paths are anchored under `~/.opencrabs/` (the same root the
/// home-anchor line teaches the agent to expand to). We list the
/// surfaces the agent reaches for repeatedly:
/// - logs (rotated daily; the agent always wants today's file)
/// - config & keys (when the user asks about settings)
/// - brain files (already enumerated elsewhere but listed here as
///   the canonical disk path)
/// - in-flight plans (per-session JSON the plan tool persists)
///
/// Keep this list short. Anything that's not a recurring user
/// question stays out; the goal is "next time you're told 'check
/// the logs' you don't grep .git/".
pub(crate) fn push_known_paths(prompt: &mut String) {
    let base = crate::config::profile::base_opencrabs_dir();
    let home = crate::config::opencrabs_home();
    prompt.push_str(&format!(
        "\nKnown paths:\n\
         - Logs: {base}/logs/opencrabs.YYYY-MM-DD (daily, today is the most relevant)\n\
         - Config: {base}/config.toml\n\
         - Keys: {base}/keys.toml\n\
         - Brain files: {home}/{{SOUL,USER,AGENTS,TOOLS,MEMORY,CODE}}.md\n\
         - Plans: {home}/agents/session/.opencrabs_plan_<session-id>.json\n\
         When the user asks to check logs, read today's file at \
         {base}/logs/opencrabs.<today UTC date>. Do NOT grep the repo \
         working directory for log files — opencrabs never writes logs there.\n",
        base = base.display(),
        home = home.display(),
    ));
}

#[cfg(test)]
#[path = "prompt_builder_tests.rs"]
mod prompt_builder_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_build_prompt_no_files() {
        let dir = TempDir::new().unwrap();
        let loader = BrainLoader::new(dir.path().to_path_buf());
        let prompt = loader.build_system_brain(None, None);

        // Should contain brain preamble even with no brain files
        assert!(prompt.contains("You are OpenCrabs"));
        assert!(prompt.contains("CRITICAL RULE"));
    }

    #[test]
    fn test_build_prompt_with_soul() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("SOUL.md"), "I am a helpful crab.").unwrap();

        let loader = BrainLoader::new(dir.path().to_path_buf());
        let prompt = loader.build_system_brain(None, None);

        assert!(prompt.contains("You are OpenCrabs"));
        assert!(prompt.contains("I am a helpful crab."));
        assert!(prompt.contains("SOUL.md"));
    }

    #[test]
    fn test_build_prompt_with_runtime_info() {
        let dir = TempDir::new().unwrap();
        let loader = BrainLoader::new(dir.path().to_path_buf());
        let info = RuntimeInfo {
            model: Some("claude-sonnet-4-20250514".to_string()),
            provider: Some("anthropic".to_string()),
            working_directory: Some("/home/user/project".to_string()),
        };
        let prompt = loader.build_system_brain(Some(&info), None);

        assert!(prompt.contains("claude-sonnet-4-20250514"));
        assert!(prompt.contains("anthropic"));
        assert!(prompt.contains("/home/user/project"));
    }

    #[test]
    fn test_skips_empty_files() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("SOUL.md"), "  \n  ").unwrap();

        let loader = BrainLoader::new(dir.path().to_path_buf());
        let prompt = loader.build_system_brain(None, None);

        // Should NOT contain SOUL.md section header for empty content
        // (the filename may appear in BRAIN_PREAMBLE tool docs, so check for the section format)
        assert!(!prompt.contains("--- SOUL.md ("));
    }
}
