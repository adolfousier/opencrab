//! Prompt Analysis and Transformation
//!
//! Analyzes user prompts to detect keywords and transforms them to include
//! explicit tool call hints for the LLM to ensure proper tool usage.

use regex::Regex;

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

    /// Build a regex from keywords (case-insensitive, word boundaries)
    fn build_keyword_regex(keywords: &[&str]) -> Regex {
        let pattern = keywords
            .iter()
            .map(|k| regex::escape(k))
            .collect::<Vec<_>>()
            .join("|");
        Regex::new(&format!(r"(?i)\b({})\b", pattern)).expect("Failed to compile keyword regex")
    }

    /// Analyze a prompt and transform it if needed
    pub fn analyze_and_transform(&self, prompt: &str) -> String {
        let mut transformations = Vec::new();
        let lower_prompt = prompt.to_lowercase();

        // Check for plan keywords
        if self.plan_regex.is_match(&lower_prompt) {
            tracing::info!("🔍 Detected PLAN intent in prompt");
            transformations.push(
                "\n\n**CRITICAL**: You MUST use the `plan` tool now! \
                DO NOT write text - CALL THE TOOL IMMEDIATELY:\n\
                1. plan(operation='create', title='...', description='...')\n\
                2. plan(operation='add_task', ...) for each task\n\
                3. plan(operation='finalize')\n\
                **START WITH THE FIRST TOOL CALL NOW!**",
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

        // If any transformations were added, append them to the prompt
        if !transformations.is_empty() {
            let hint_section = transformations.join("");
            format!("{}{}", prompt, hint_section)
        } else {
            prompt.to_string()
        }
    }
}

impl Default for PromptAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plan_detection() {
        let analyzer = PromptAnalyzer::new();

        let prompt = "make a plan for implementing JWT authentication";
        let result = analyzer.analyze_and_transform(prompt);
        assert!(result.contains("TOOL HINT"));
        assert!(result.contains("`plan` tool"));
    }

    #[test]
    fn test_read_file_detection() {
        let analyzer = PromptAnalyzer::new();

        let prompt = "read the file src/main.rs and explain it";
        let result = analyzer.analyze_and_transform(prompt);
        assert!(result.contains("TOOL HINT"));
        assert!(result.contains("`read_file` tool"));
    }

    #[test]
    fn test_search_detection() {
        let analyzer = PromptAnalyzer::new();

        let prompt = "search for the function getUserData";
        let result = analyzer.analyze_and_transform(prompt);
        assert!(result.contains("TOOL HINT"));
        assert!(result.contains("`grep` tool"));
    }

    #[test]
    fn test_multiple_detections() {
        let analyzer = PromptAnalyzer::new();

        let prompt = "read file config.toml and make a plan to update it";
        let result = analyzer.analyze_and_transform(prompt);
        assert!(result.contains("`plan` tool"));
        assert!(result.contains("`read_file` tool"));
    }

    #[test]
    fn test_no_detection() {
        let analyzer = PromptAnalyzer::new();

        let prompt = "explain how to use rust";
        let result = analyzer.analyze_and_transform(prompt);
        assert_eq!(result, prompt);
    }

    #[test]
    fn test_case_insensitive() {
        let analyzer = PromptAnalyzer::new();

        let prompt = "MAKE A PLAN for this feature";
        let result = analyzer.analyze_and_transform(prompt);
        assert!(result.contains("TOOL HINT"));
        assert!(result.contains("`plan` tool"));
    }

    #[test]
    fn test_web_search_detection() {
        let analyzer = PromptAnalyzer::new();

        let prompt = "search the web for rust async best practices";
        let result = analyzer.analyze_and_transform(prompt);
        assert!(result.contains("TOOL HINT"));
        assert!(result.contains("`web_search` tool"));
    }

    #[test]
    fn test_bash_detection() {
        let analyzer = PromptAnalyzer::new();

        let prompt = "run command cargo build";
        let result = analyzer.analyze_and_transform(prompt);
        assert!(result.contains("TOOL HINT"));
        assert!(result.contains("`bash` tool"));
    }
}
