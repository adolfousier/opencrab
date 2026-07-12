//! Prompt Analysis and Transformation (shared soft-nudge)
//!
//! Analyzes natural-language user prompts to detect keywords and appends
//! explicit tool call hints for the LLM to ensure proper tool usage.
//!
//! Shared by every surface (TUI and Telegram). Hints are LLM-only: they are
//! appended to the agent input string and must never reach user-visible
//! display paths (Telegram `display_text`, TUI chat bubbles). Slash commands
//! and skill/user-command expansions are never analyzed: soft-nudge means
//! the USER said keyword-shaped language, not that a skill author wrote it.

use regex::Regex;
use std::sync::LazyLock;

/// Keywords that trigger plan tool usage
const PLAN_KEYWORDS: &[&str] = &[
    "make a plan",
    "create a plan",
    "plan for",
    "plan to implement",
    "plan out",
    "planning",
    "create plan",
    "make plan",
];

/// Keywords that trigger read_file tool usage
const READ_FILE_KEYWORDS: &[&str] = &[
    "read file",
    "read the file",
    "show me file",
    "show me the file",
    "show file content",
    "what's in",
    "what is in",
    "display file",
    "view file",
    "look at file",
    "check file",
];

/// Keywords that trigger grep/search tool usage
const SEARCH_KEYWORDS: &[&str] = &[
    "search for",
    "find",
    "look for",
    "grep",
    "search code",
    "find in files",
    "search in",
    "where is",
    "locate",
];

/// Keywords that trigger write_file tool usage
const WRITE_FILE_KEYWORDS: &[&str] = &[
    "create file",
    "create a file",
    "write file",
    "write to file",
    "make a file",
    "make file",
    "new file",
];

/// Keywords that trigger edit_file tool usage
const EDIT_FILE_KEYWORDS: &[&str] = &[
    "edit file",
    "modify file",
    "update file",
    "change file",
    "fix in file",
    "update the file",
    "modify the file",
];

/// Keywords that trigger bash tool usage
const BASH_KEYWORDS: &[&str] = &[
    "run command",
    "execute command",
    "run shell",
    "shell command",
    "terminal command",
    "bash command",
];

/// Keywords that trigger web_search tool usage
const WEB_SEARCH_KEYWORDS: &[&str] = &[
    "search online",
    "search the web",
    "google",
    "search internet",
    "find online",
    "look up online",
    "web search",
];

static SHARED: LazyLock<PromptAnalyzer> = LazyLock::new(PromptAnalyzer::new);

/// True when an utterance is natural-language chat the analyzer may inspect.
/// Slash commands and system triggers are never soft-nudged.
pub fn is_natural_chat(text: &str) -> bool {
    let t = text.trim_start();
    !t.starts_with('/') && !t.to_ascii_lowercase().starts_with("[system")
}

/// Prompt analyzer that detects keywords and suggests tool usage
pub struct PromptAnalyzer {
    plan_regex: Regex,
    read_file_regex: Regex,
    search_regex: Regex,
    write_file_regex: Regex,
    edit_file_regex: Regex,
    bash_regex: Regex,
    web_search_regex: Regex,
}

impl PromptAnalyzer {
    /// Create a new prompt analyzer
    pub fn new() -> Self {
        Self {
            plan_regex: Self::build_keyword_regex(PLAN_KEYWORDS),
            read_file_regex: Self::build_keyword_regex(READ_FILE_KEYWORDS),
            search_regex: Self::build_keyword_regex(SEARCH_KEYWORDS),
            write_file_regex: Self::build_keyword_regex(WRITE_FILE_KEYWORDS),
            edit_file_regex: Self::build_keyword_regex(EDIT_FILE_KEYWORDS),
            bash_regex: Self::build_keyword_regex(BASH_KEYWORDS),
            web_search_regex: Self::build_keyword_regex(WEB_SEARCH_KEYWORDS),
        }
    }

    /// Process-wide shared instance, so the keyword regexes compile once.
    pub fn shared() -> &'static PromptAnalyzer {
        &SHARED
    }

    /// Build a regex from keywords (case-insensitive, word boundaries)
    fn build_keyword_regex(keywords: &[&str]) -> Regex {
        let pattern = keywords
            .iter()
            .map(|k| regex::escape(k))
            .collect::<Vec<_>>()
            .join("|");
        Regex::new(&format!(r"(?i)\b({})\b", pattern)).expect("Failed to compile keyword regex")
    }

    /// Return the hint section for a prompt, or `None` when no keyword
    /// family matches. Callers append this to the LLM agent input only,
    /// never to user-visible display text.
    pub fn hints_for(&self, prompt: &str) -> Option<String> {
        let mut transformations = Vec::new();
        let lower_prompt = prompt.to_lowercase();

        // Check for plan keywords: design track (the user said "plan", so
        // they get a reviewable SESSION PLAN and an Approve step, not a
        // checklist that starts executing on its own).
        if self.plan_regex.is_match(&lower_prompt) {
            tracing::info!("🔍 Detected PLAN intent in prompt");
            transformations.push(
                "\n\n**CRITICAL**: The user wants a PLAN — enter the design track NOW:\n\
                1. Explore first if needed (reads, search, bash are available pre-init).\n\
                2. plan(operation='init', mode='design', title='...') creates the \
                SESSION PLAN .md and returns its absolute path.\n\
                3. Write the design INTO that .md (write_file/edit_file on that exact \
                path — the only writable file) using the template: ## Context with \
                **Problem:** / **Target state:** / **Intent:**, then numbered \
                ## Implementation steps.\n\
                4. STOP and wait for the user to APPROVE the plan (/execute). Do NOT \
                call start, do NOT edit project files, do NOT paste the plan in chat.\n\
                The checklist is seeded automatically after Approve. Valid operations \
                are EXACTLY: init, add_tasks, add_task, start, complete. There is NO \
                'create' and NO 'finalize' operation, never call those.",
            );
        }

        // Check for read_file keywords
        if self.read_file_regex.is_match(&lower_prompt) {
            tracing::info!("🔍 Detected READ_FILE intent in prompt");
            transformations
                .push("\n\n**TOOL HINT**: Use the `read_file` tool to read the contents of files.");
        }

        // Check for search keywords
        if self.search_regex.is_match(&lower_prompt) {
            tracing::info!("🔍 Detected SEARCH/GREP intent in prompt");
            transformations.push(
                "\n\n**TOOL HINT**: Use the `grep` tool to search for patterns in files, \
                or use `glob` to find files by pattern.",
            );
        }

        // Check for write_file keywords
        if self.write_file_regex.is_match(&lower_prompt) {
            tracing::info!("🔍 Detected WRITE_FILE intent in prompt");
            transformations
                .push("\n\n**TOOL HINT**: Use the `write_file` tool to create new files.");
        }

        // Check for edit_file keywords
        if self.edit_file_regex.is_match(&lower_prompt) {
            tracing::info!("🔍 Detected EDIT_FILE intent in prompt");
            transformations
                .push("\n\n**TOOL HINT**: Use the `edit_file` tool to modify existing files.");
        }

        // Check for bash keywords
        if self.bash_regex.is_match(&lower_prompt) {
            tracing::info!("🔍 Detected BASH intent in prompt");
            transformations
                .push("\n\n**TOOL HINT**: Use the `bash` tool to execute shell commands.");
        }

        // Check for web_search keywords
        if self.web_search_regex.is_match(&lower_prompt) {
            tracing::info!("🔍 Detected WEB_SEARCH intent in prompt");
            transformations.push(
                "\n\n**TOOL HINT**: Use the `web_search` tool to search the internet for \
                real-time information.",
            );
        }

        if transformations.is_empty() {
            None
        } else {
            Some(transformations.join(""))
        }
    }

    /// Whether the prompt matches the plan keyword family. Callers use
    /// this to set the durable `pre_init_editing` flag when a plan-shaped
    /// message arrives on natural-language chat.
    pub fn plan_intent(&self, prompt: &str) -> bool {
        self.plan_regex.is_match(&prompt.to_lowercase())
    }

    /// Analyze a prompt and transform it if needed
    pub fn analyze_and_transform(&self, prompt: &str) -> String {
        match self.hints_for(prompt) {
            Some(hint_section) => format!("{}{}", prompt, hint_section),
            None => prompt.to_string(),
        }
    }
}

impl Default for PromptAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}
