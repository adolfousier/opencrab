use super::builder::AgentService;
use crate::brain::agent::context::AgentContext;
use crate::brain::agent::error::{AgentError, Result};
use crate::brain::provider::{ContentBlock, LLMRequest, Message, Provider};
use crate::services::{MessageService, SessionService};
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

impl AgentService {
    /// Helper to prepare message context for LLM requests
    ///
    /// This extracts the common setup logic shared between send_message() and
    /// send_message_streaming() to reduce code duplication.
    pub(super) async fn prepare_message_context(
        &self,
        session_id: Uuid,
        user_message: String,
        model: Option<String>,
    ) -> Result<(String, LLMRequest, MessageService, SessionService)> {
        // Get or create session
        let session_service = SessionService::new(self.context.clone());
        let _session = session_service
            .get_session(session_id)
            .await
            .map_err(|e| AgentError::Database(e.to_string()))?
            .ok_or(AgentError::SessionNotFound(session_id))?;

        // Load conversation context with budget-aware message trimming
        let message_service = MessageService::new(self.context.clone());
        let all_db_messages = message_service
            .list_messages_for_session(session_id)
            .await
            .map_err(|e| AgentError::Database(e.to_string()))?;

        let model_name = model.unwrap_or_else(|| {
            self.provider_for_session(session_id)
                .default_model()
                .to_string()
        });
        let context_window = self.context_limit_for_session(session_id);

        // Load from last compaction point — no arbitrary trimming
        let db_messages = Self::messages_from_last_compaction(all_db_messages);

        let mut context =
            AgentContext::from_db_messages(session_id, db_messages, context_window as usize);

        // Add system brain if available (count its tokens for accurate tracking)
        if let Some(brain) = &self.default_system_brain {
            context.token_count += AgentContext::estimate_tokens(brain);
            context.system_brain = Some(brain.clone());
        }

        // Add user message
        let user_msg = Message::user(user_message.clone());
        context.add_message(user_msg);

        // Save user message to database
        message_service
            .create_message(session_id, "user".to_string(), user_message)
            .await
            .map_err(|e| AgentError::Database(e.to_string()))?;

        // Build base LLM request
        let request = LLMRequest::new(model_name.clone(), context.messages.clone())
            .with_max_tokens(self.max_tokens);

        // Surface a small "Recently accessed" anchor section so the
        // agent re-uses real paths from prior sessions / pre-compaction
        // turns instead of hallucinating directory layouts. Filtered
        // against the live messages so we don't double-list paths the
        // agent just touched in this same session.
        let working_directory = self.get_working_directory();
        let recent_paths = self.recent_paths_for_dir(&working_directory).await;
        let augmented_system = Self::augment_system_with_recent_paths(
            context.system_brain,
            &recent_paths,
            &context.messages,
        );

        let mut request = if let Some(system) = augmented_system {
            request.with_system(system)
        } else {
            request
        };

        // Pass working directory so proxy-aware providers can forward it
        request.working_directory = Some(working_directory.to_string_lossy().to_string());
        request.session_id = Some(session_id);

        Ok((model_name, request, message_service, session_service))
    }

    /// Append a "Recently accessed" anchor section to `system_brain`,
    /// listing only the paths from the persistent recent-paths store
    /// that don't already appear verbatim in any of the live messages.
    /// Returns `base` unchanged when there's nothing to surface — keeps
    /// the prompt clean during normal uncompacted runs where the literal
    /// tool_call/tool_result blocks already mention the path.
    pub(crate) fn augment_system_with_recent_paths(
        base: Option<String>,
        recent_paths: &[String],
        messages: &[Message],
    ) -> Option<String> {
        if recent_paths.is_empty() {
            return base;
        }
        let context_blob: String = messages
            .iter()
            .flat_map(|m| m.content.iter())
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.clone()),
                ContentBlock::ToolUse { input, .. } => Some(input.to_string()),
                ContentBlock::ToolResult { content, .. } => Some(content.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        let context_for_match = context_blob.to_lowercase();

        let surviving: Vec<&String> = recent_paths
            .iter()
            .filter(|p| !context_for_match.contains(&p.to_lowercase()))
            .collect();
        if surviving.is_empty() {
            return base;
        }
        let mut out = base.unwrap_or_default();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(
            "\n--- Recently accessed in this project ---\n\
             (Real paths previously confirmed by read/edit/grep/ls. Prefer these as anchors \
             over guessing from naming conventions.)\n",
        );
        for p in surviving {
            out.push_str("  - ");
            out.push_str(p);
            out.push('\n');
        }
        Some(out)
    }

    /// Load messages from the last compaction point forward.
    ///
    /// Finds the last message containing the `[CONTEXT COMPACTION` marker and
    /// returns only messages from that point onward. If no compaction marker
    /// exists, returns all messages. This ensures restarts pick up exactly
    /// where compaction left off — no arbitrary trimming.
    pub fn messages_from_last_compaction(
        all_messages: Vec<crate::db::models::Message>,
    ) -> Vec<crate::db::models::Message> {
        const COMPACTION_MARKER: &str = "[CONTEXT COMPACTION";

        // Walk backward to find the last compaction marker
        let compaction_idx = all_messages
            .iter()
            .rposition(|msg| msg.content.contains(COMPACTION_MARKER));

        if let Some(idx) = compaction_idx {
            let kept = all_messages.len() - idx;
            tracing::info!(
                "Found compaction marker at message {}/{} — loading {} messages from compaction point",
                idx,
                all_messages.len(),
                kept,
            );
            all_messages[idx..].to_vec()
        } else {
            all_messages
        }
    }

    /// Build a "recovered brain" context string from key brain files.
    ///
    /// After compaction wipes the conversation history, this restores the agent's
    /// core identity, user context, tool documentation, and coding standards so it
    /// doesn't wake up with only a lossy LLM summary.
    ///
    /// Full files injected (~1-2k tokens total):
    /// - SOUL.md — personality, tone, hard rules
    /// - USER.md — who the human is, preferences
    /// - TOOLS.md — environment-specific tool notes
    ///
    /// CODE.md is injected as a compact summary only. Before ANY code task the
    /// agent MUST fetch the full file. Non-code tasks can ignore this section.
    ///
    /// Skipped: MEMORY.md (summary replaces it), BOOT/BOOTSTRAP/HEARTBEAT (rarely
    /// needed mid-task), SECURITY.md/AGENTS.md (loaded on demand if flagged in
    /// summary), IDENTITY.md (only for cron/social sessions).
    fn build_recovered_brain_context() -> String {
        use std::path::PathBuf;

        const CODE_MD_SUMMARY: &str =
"## CODE.md — Coding Standards (SUMMARY)
**Full file: ~/.opencrabs/CODE.md — use `load_brain_file(\"CODE.md\")` to read it before writing ANY code.**
If you are NOT doing code tasks, ignore this section entirely.

Best practices:
- Rust first. Always. (heyiolo is built in Dart/Swift — those are the only exceptions)
- Max 500 lines per file, target 100-250. Split without hesitation.
- Types in types.rs, handlers in handler.rs. One responsibility per file.
- Tests in `src/tests/<module>_test.rs` — never inline in source.
- `cargo clippy --all-features` + `cargo test --all-features` before every commit.
- No unwraps on user data, no dead code, no suppressing warnings.
- No #[allow()] unless you can defend why the lint is wrong.
- No unsafe without a soundness comment.
- Validate all external input. No hardcoded secrets. Sanitize output.
- Never give up on a problem. Never suppress errors.
- Git diff before commit — match the request exactly, no more, no less.

**CRITICAL: Before handling ANY code task, fetch full CODE.md:**
Use the `load_brain_file` tool with name=\"CODE.md\" — reads from ~/.opencrabs/CODE.md.
The summary above is NOT sufficient for implementation work.
";

        let full_files = [
            ("SOUL.md", "personality"),
            ("USER.md", "user profile"),
            ("TOOLS.md", "tool notes"),
        ];

        let opencrabs_home = crate::config::opencrabs_home();
        let mut result = String::new();

        for (filename, label) in full_files {
            let path: PathBuf = opencrabs_home.join(filename);
            if let Ok(content) = std::fs::read_to_string(&path) {
                let trimmed = content.trim();
                if !trimmed.is_empty() {
                    result.push_str(&format!(
                        "--- {} ({}) ---\n{}\n\n",
                        filename, label, trimmed
                    ));
                }
            }
        }

        result.push_str(CODE_MD_SUMMARY);

        if result.is_empty() {
            String::from("[No brain files found — agent context limited]\n\n")
        } else {
            format!(
                "[RECOVERED BRAIN CONTEXT — these files define your identity, the user, your tools, and your coding standards. They take priority over any contradictory inference from the summary.]\n\n{}\n",
                result
            )
        }
    }

    /// Synchronous compaction: compute a summary and apply it to `context` in place.
    /// Used by the manual `/compact` command and the two emergency callsites that
    /// recover from "context too large" provider errors. The async path used by
    /// `enforce_context_budget` does NOT go through here — it spawns
    /// `compute_compaction_summary` directly and applies the result via
    /// `apply_compaction_summary` once the LLM call finishes.
    pub(super) async fn compact_context(
        &self,
        session_id: Uuid,
        context: &mut AgentContext,
        model_name: &str,
        cancel_token: Option<&CancellationToken>,
    ) -> Result<String> {
        let provider = self.provider_for_session(session_id);
        let cancel = cancel_token.cloned().unwrap_or_default();

        let summary = Self::compute_compaction_summary(
            provider,
            session_id,
            context.messages.clone(),
            context.token_count,
            context.max_tokens,
            context.usage_percentage(),
            model_name.to_string(),
            self.max_tokens,
            self.get_working_directory(),
            self.auto_approve_tools,
            cancel,
        )
        .await?;

        Self::apply_compaction_summary(context, &summary);
        Ok(summary)
    }

    /// Compute a compaction summary from a snapshot of messages.
    ///
    /// This is the LLM-facing half of compaction. It does not touch any live
    /// session state — it operates entirely on the cloned snapshot — so it
    /// is safe to call from a background `tokio::spawn` task while the agent
    /// keeps appending new messages to the live context.
    ///
    /// Returns the raw summary text. Callers that want to apply it to a live
    /// context should call `apply_compaction_summary` once the future resolves.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn compute_compaction_summary(
        provider: Arc<dyn Provider>,
        session_id: Uuid,
        snapshot_messages: Vec<Message>,
        snapshot_token_count: usize,
        snapshot_max_tokens: usize,
        snapshot_usage_pct: f64,
        model_name: String,
        max_output_tokens: u32,
        working_directory: PathBuf,
        auto_approve_tools: bool,
        cancel: CancellationToken,
    ) -> Result<String> {
        let remaining_budget = snapshot_max_tokens.saturating_sub(snapshot_token_count);

        // Skip any leading user messages that consist only of ToolResult blocks —
        // they are orphaned (their tool_use was removed by a prior trim) and would
        // cause the API to reject the request with a 400.
        let start = snapshot_messages
            .iter()
            .position(|m| {
                !(m.role == crate::brain::provider::Role::User
                    && !m.content.is_empty()
                    && m.content.iter().all(|b| {
                        matches!(b, crate::brain::provider::ContentBlock::ToolResult { .. })
                    }))
            })
            .unwrap_or(snapshot_messages.len());

        // Reserve room for the summarizer's OUTPUT budget (8k) + prompt (~1k).
        let output_reserve = 8_000usize + 1_000usize;
        let max_input_budget = snapshot_max_tokens.saturating_sub(output_reserve);
        let all_msgs = &snapshot_messages[start..];
        let mut running_tokens = 0usize;
        let msgs_to_include: Vec<&Message> = all_msgs
            .iter()
            .rev()
            .take_while(|m| {
                let t = AgentContext::estimate_tokens_static(m);
                if running_tokens + t <= max_input_budget {
                    running_tokens += t;
                    true
                } else {
                    tracing::warn!(
                        "Compaction: dropping oldest messages to fit input budget ({}/{} tokens used)",
                        running_tokens,
                        max_input_budget,
                    );
                    false
                }
            })
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        tracing::info!(
            "Compaction: sending {} / {} messages to summarizer ({} / {} input tokens, reserving {} for output)",
            msgs_to_include.len(),
            all_msgs.len(),
            running_tokens,
            snapshot_max_tokens,
            output_reserve,
        );

        let mut summary_messages: Vec<Message> = msgs_to_include.into_iter().cloned().collect();

        let compaction_prompt = format!(
            "CRITICAL: The context window is at {:.0}% capacity ({} / {} tokens, {} tokens remaining). \
             The conversation must be compacted NOW.\n\n\
             You are creating a COMPREHENSIVE CONTINUATION DOCUMENT. After compaction, a fresh agent \
             instance will wake up with ONLY this summary as context. It must be able to continue \
             working immediately without asking the user what to do.\n\n\
             Analyze the ENTIRE conversation chronologically and produce the following:\n\n\
             ## 1. Chronological Analysis\n\
             Walk through every task the user requested, in order. For each task include:\n\
             - What was requested\n\
             - What was done (with exact file paths and line numbers where relevant)\n\
             - Exact code snippets for any changes made (show before/after when applicable)\n\
             - Whether it was completed, committed, pushed, or still pending\n\n\
             ## 2. Files Modified\n\
             List EVERY file that was created, edited, read, or discussed. For each file include:\n\
             - Full file path\n\
             - What was changed and why\n\
             - Key code snippets showing the current state of changes\n\
             - Whether the change is committed or uncommitted\n\n\
             ## 3. User Preferences & Constraints\n\
             List EVERY preference, constraint, or strong reaction from the user. Include:\n\
             - Things the user explicitly said to NEVER do (with their exact words if they were emphatic)\n\
             - Workflow preferences (commit style, release process, tool choices)\n\
             - Technical constraints or architectural decisions\n\
             - Any corrections the user made to your work\n\n\
             ## 4. Errors & Corrections\n\
             Every error encountered, every mistake made, and how each was resolved. Include:\n\
             - Exact error messages when available\n\
             - What caused the error\n\
             - The fix applied\n\
             - User reactions to mistakes (so the agent avoids repeating them)\n\n\
             ## 5. All User Messages\n\
             Summarize every user message in order, capturing their intent and exact wording \
             for important instructions. This is critical for understanding the user's communication \
             style and expectations.\n\n\
             ## 6. Pending Tasks\n\
             List everything that is NOT yet done:\n\
             - Uncommitted changes\n\
             - Tasks mentioned but not started\n\
             - Investigations in progress\n\
             - Next steps the user expects\n\n\
             ## 7. Current Work\n\
             What was the agent doing RIGHT BEFORE this compaction? What is the immediate next action? \
             The fresh agent must pick up exactly where this left off.\n\n\
             ## 8. Recovery Playbook\n\
             The fresh agent has these tools available to recover any missing context:\n\
             - `session_search` — search past conversation messages in this session by keyword\n\
             - `memory_search` — search daily memory logs and indexed knowledge\n\
             - `load_brain_file` — reload brain files (SOUL.md, TOOLS.md, USER.md, etc.) for identity/preferences\n\
             - `read_file` / `glob` / `grep` — read any file, search by pattern, search file contents\n\
             - `bash` — run shell commands (git status, git log, git diff, etc.)\n\
             - `ls` — list directory contents\n\
             - `gh` — GitHub CLI for ALL GitHub operations (repos, releases, issues, PRs). \
             NEVER use HTTP requests to GitHub — always use `gh` CLI.\n\n\
             Write a SPECIFIC recovery plan: which tools to call with which arguments to get back \
             up to speed. Example: \"Run `git status` and `git diff` to see uncommitted changes, \
             then `read_file src/main.rs` to verify the current state of the fix, then \
             `session_search 'vision fallback'` to recover details from the investigation.\"\n\
             Be concrete — include actual file paths, search queries, and commands.\n\n\
             ## 9. Next Step\n\
             State the single most important thing the agent should do when it wakes up. \
             If the task is clear, continue immediately. If ambiguous, ask the user ONE focused \
             follow-up question.\n\n\
             ## 10. Continuation Message\n\
             Write a SHORT, punchy message (2-4 sentences) that the agent will say to the user \
             right after waking up from compaction. This message MUST:\n\
             - Reference SPECIFIC things from the conversation (file names, user quotes, inside jokes, \
             frustrations, wins) — prove the agent remembers everything\n\
             - Mention what was just accomplished and what's next in a way that feels alive and engaged\n\
             - Match the user's energy and communication style from the conversation\n\
             - Be creative, surprising, maybe funny — make the user think \"holy shit it remembers\"\n\
             - End with a clear action: what the agent is about to do next or a specific question\n\
             DO NOT be generic. DO NOT say \"I'm ready to continue.\" Reference actual conversation details \
             that only someone who was there would know.\n\n\
             Tool approval status: {}\n\n\
             BE EXHAUSTIVE. This is not a summary — it is a complete knowledge transfer. \
             Include code snippets, exact paths, user quotes, error messages. \
             The fresh agent has ZERO context beyond what you write here.",
            snapshot_usage_pct,
            snapshot_token_count,
            snapshot_max_tokens,
            remaining_budget,
            if auto_approve_tools {
                "AUTO-APPROVE ON (tools run freely)"
            } else {
                "AUTO-APPROVE OFF — tool approval is REQUIRED for every tool call"
            },
        );

        summary_messages.push(Message::user(compaction_prompt));

        // Never send a {provider, model} pair the user didn't configure.
        // If the requested model isn't supported by this provider, remap to
        // the provider's own default — same invariant `stream_complete` enforces.
        let mut effective_model = model_name;
        let supported = provider.supported_models();
        if !supported.is_empty() && !supported.iter().any(|m| m == &effective_model) {
            let remapped = provider.default_model().to_string();
            tracing::warn!(
                "compute_compaction_summary: provider '{}' does not support model '{}' — remapping to '{}'",
                provider.name(),
                effective_model,
                remapped,
            );
            effective_model = remapped;
        }

        let mut request = LLMRequest::new(effective_model, summary_messages)
            .with_max_tokens(max_output_tokens)
            .with_system(
                "You are a continuation document generator. Your job is to create an exhaustive, \
                 detailed knowledge transfer document from a conversation so that a fresh AI agent can \
                 continue the work seamlessly. You must capture every file path, code snippet, user preference, \
                 error, and pending task. The agent reading your output will have ZERO prior context — \
                 your document is its entire memory. Be thorough to the point of being verbose. \
                 Missing a single detail could cause the agent to repeat mistakes or violate user preferences."
                    .to_string(),
            );
        request.working_directory = Some(working_directory.to_string_lossy().to_string());
        request.session_id = Some(session_id);

        // Non-streaming call so no compaction text leaks to the TUI in the
        // background-spawn case. `cancel` aborts the request mid-flight if the
        // caller signals (e.g. 90% hard-truncate firing on the same session).
        let response = tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                tracing::info!("Compaction cancelled before completion");
                return Err(AgentError::Cancelled);
            }
            r = provider.complete(request) => r.map_err(AgentError::Provider)?,
        };

        let summary = Self::extract_text_from_response(&response);

        if let Err(e) = Self::save_compaction_summary_to_memory(&summary).await {
            tracing::warn!("Failed to save compaction summary to daily log: {}", e);
        }

        // Index the updated memory file in the background so memory_search picks it up.
        let memory_path = crate::config::opencrabs_home()
            .join("memory")
            .join(format!("{}.md", chrono::Local::now().format("%Y-%m-%d")));
        tokio::spawn(async move {
            if let Ok(store) = crate::memory::get_store() {
                let _ = crate::memory::index_file(store, &memory_path).await;
            }
        });

        Ok(summary)
    }

    /// Apply a previously-computed compaction summary to a live `AgentContext`.
    ///
    /// Builds the recovered-brain preamble plus a snapshot of the most recent
    /// messages, then calls `AgentContext::compact_with_summary` to do the
    /// in-place swap (replace older messages with the summary, keep the recent
    /// tail within 55% of the window).
    pub(super) fn apply_compaction_summary(context: &mut AgentContext, summary: &str) {
        let recent_snapshot = Self::format_recent_messages(&context.messages, 8);
        let brain_context = Self::build_recovered_brain_context();
        let summary_with_context = if recent_snapshot.is_empty() {
            format!("{}\n\n{}", brain_context, summary)
        } else {
            format!(
                "{}\n\n{}\n\n## Recent Message Pairs (pre-compaction snapshot)\n\
                 The following are the last messages before compaction — use them to \
                 understand the current task state and decide what context to reload.\n\n{}",
                brain_context, summary, recent_snapshot
            )
        };

        // After compaction, the summary IS the conversation — it's prepended
        // as a single user message and the agent picks up from there. We do
        // NOT preserve a raw pre-compaction tail: the summary already embeds
        // the recent-snapshot prose, so keeping the raw tail on top would
        // just duplicate ~half the window and defeat the whole purpose of
        // compacting. Pass 0 so `compact_with_summary` clears everything
        // and prepends just the summary.
        context.compact_with_summary(summary_with_context, 0);

        tracing::info!(
            "Context compacted: now at {:.0}% ({} tokens)",
            context.usage_percentage(),
            context.token_count
        );
    }

    /// Format the last N messages into a human-readable snapshot for post-compaction context.
    /// Truncates long tool results to keep the snapshot concise.
    pub(crate) fn format_recent_messages(messages: &[Message], n: usize) -> String {
        use crate::brain::provider::{ContentBlock, Role};

        let start = messages.len().saturating_sub(n);
        let mut lines = Vec::new();

        for msg in &messages[start..] {
            let role_label = match msg.role {
                Role::User => "**User**",
                Role::Assistant => "**Assistant**",
                Role::System => "**System**",
            };

            for block in &msg.content {
                match block {
                    ContentBlock::Text { text } => {
                        // Truncate very long text blocks to ~500 bytes
                        let display = if text.len() > 500 {
                            let end = text.floor_char_boundary(500);
                            format!("{}… [truncated]", &text[..end])
                        } else {
                            text.clone()
                        };
                        lines.push(format!("{}: {}", role_label, display));
                    }
                    ContentBlock::ToolUse { name, input, .. } => {
                        let input_preview = {
                            let s = input.to_string();
                            if s.len() > 200 {
                                let end = s.floor_char_boundary(200);
                                format!("{}…", &s[..end])
                            } else {
                                s
                            }
                        };
                        lines.push(format!(
                            "{}: [tool_use: {}({})]",
                            role_label, name, input_preview
                        ));
                    }
                    ContentBlock::ToolResult { content, .. } => {
                        let display = if content.len() > 300 {
                            let end = content.floor_char_boundary(300);
                            format!("{}… [truncated]", &content[..end])
                        } else {
                            content.clone()
                        };
                        lines.push(format!("{}: [tool_result: {}]", role_label, display));
                    }
                    ContentBlock::Image { .. } => {
                        lines.push(format!("{}: [image]", role_label));
                    }
                    ContentBlock::Thinking { thinking, .. } => {
                        if !thinking.is_empty() {
                            let display = if thinking.len() > 300 {
                                let end = thinking.floor_char_boundary(300);
                                format!("{}… [truncated]", &thinking[..end])
                            } else {
                                thinking.clone()
                            };
                            lines.push(format!("{}: [thinking: {}]", role_label, display));
                        }
                    }
                }
            }
        }

        lines.join("\n")
    }

    /// Save a compaction summary to a daily memory log at `~/.opencrabs/memory/YYYY-MM-DD.md`.
    ///
    /// Multiple compactions per day append to the same file. The brain workspace's
    /// `MEMORY.md` is left untouched — it stays as user-curated durable memory.
    pub(super) async fn save_compaction_summary_to_memory(
        summary: &str,
    ) -> std::result::Result<(), String> {
        let memory_dir = crate::config::opencrabs_home().join("memory");

        std::fs::create_dir_all(&memory_dir)
            .map_err(|e| format!("Failed to create memory directory: {}", e))?;

        let date = chrono::Local::now().format("%Y-%m-%d");
        let memory_path = memory_dir.join(format!("{}.md", date));

        // Read existing content (if any — multiple compactions per day stack)
        let existing = std::fs::read_to_string(&memory_path).unwrap_or_default();

        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let new_content = format!(
            "{}\n\n---\n\n## Auto-Compaction Summary ({})\n\n{}\n",
            existing.trim(),
            timestamp,
            summary
        );

        std::fs::write(&memory_path, new_content.trim_start())
            .map_err(|e| format!("Failed to write daily memory log: {}", e))?;

        tracing::info!("Saved compaction summary to {}", memory_path.display());
        Ok(())
    }
}
