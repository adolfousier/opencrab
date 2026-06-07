//! Tool input sanitization for safe display in approval messages and UI.
//!
//! When the agent calls tools like `http_request` or `bash`, the inputs may
//! contain API keys, Authorization headers, passwords, or tokens. This module
//! redacts those values before they are shown to users in Telegram/WhatsApp
//! approval dialogs or the TUI, while preserving enough context (field names,
//! non-sensitive values) for the user to understand what the tool is doing.

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{Map, Value};

/// Field name patterns (case-insensitive) whose values are always redacted.
const SENSITIVE_KEYS: &[&str] = &[
    "authorization",
    "api_key",
    "apikey",
    "api-key",
    "x-api-key",
    "x-auth-token",
    "x-access-token",
    "token",
    "secret",
    "password",
    "passwd",
    "pass",
    "credential",
    "credentials",
    "access_token",
    "refresh_token",
    "client_secret",
    "private_key",
    "auth",
    "bearer",
];

/// Regex-like patterns in bash commands to redact inline secrets.
/// Each tuple is (prefix_to_find, chars_to_keep_after_prefix).
/// We redact the rest of the token after these prefixes.
const COMMAND_SENSITIVE_PATTERNS: &[&str] = &[
    "bearer ",
    "authorization: ",
    "x-api-key: ",
    "x-auth-token: ",
    "api_key=",
    "apikey=",
    "api-key=",
    "token=",
    "secret=",
    "password=",
    "passwd=",
    "access_token=",
    // Environment variable suffixes that typically hold secrets
    "_pass=",
    "_password=",
    "_passwd=",
    "_secret=",
    "_token=",
    "_key=",
    "_apikey=",
    "_api_key=",
    "_credential=",
    "_credentials=",
    "_auth=",
];

/// Returns true if a JSON object key looks like it holds a sensitive value.
fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_lowercase();
    SENSITIVE_KEYS
        .iter()
        .any(|&pat| lower == pat || lower.contains(pat))
}

/// Redact sensitive values from a bash command string.
/// Handles patterns like:
///   -H "Authorization: Bearer sk-xxx"
///   --header "X-Api-Key: abc123"
///   https://user:password@host/path
///   api_key=abc123
///
/// `pub(crate)` so the TUI's one-line tool-call summary can redact a
/// command before display — the expanded view already redacts via
/// `redact_tool_input`, this closes the same leak in the collapsed line.
pub(crate) fn redact_command(cmd: &str) -> String {
    let mut result = cmd.to_string();

    // Redact URL passwords: https://user:PASSWORD@host → https://user:[REDACTED]@host
    // Simple approach: find ://word:word@ patterns
    if let Some(at_pos) = result.find("://") {
        let rest = &result[at_pos + 3..];
        if let Some(at_sign) = rest.find('@')
            && let Some(colon) = rest[..at_sign].find(':')
        {
            let pass_start = at_pos + 3 + colon + 1;
            let pass_end = at_pos + 3 + at_sign;
            if pass_start < pass_end && pass_end <= result.len() {
                result.replace_range(pass_start..pass_end, "[REDACTED]");
            }
        }
    }

    // Redact inline header values and query params (case-insensitive).
    //
    // All COMMAND_SENSITIVE_PATTERNS are pure ASCII. We use find_case_insensitive
    // (below) instead of to_lowercase() to avoid the byte-offset mismatch problem:
    // to_lowercase() can expand Unicode chars (e.g. 'İ' → 'i̇', 2→3 bytes), causing
    // positions from the lowered string to misalign with the original string, which
    // in turn causes wrong redaction or panic on slicing.
    for pattern in COMMAND_SENSITIVE_PATTERNS {
        let mut search_start = 0;
        while let Some(abs_pos) = find_case_insensitive(&result[search_start..], pattern) {
            let true_pos = search_start + abs_pos;
            let after = true_pos + pattern.len();
            if after > result.len() {
                break;
            }
            // Find end of the secret: whitespace, quote, or end of string
            let secret_end = result[after..]
                .find(['"', '\'', ' ', '&', '\n'])
                .map(|p| after + p)
                .unwrap_or(result.len());
            if secret_end > after {
                result.replace_range(after..secret_end, "[REDACTED]");
            }
            // Advance past where we just redacted to avoid infinite loop
            search_start = after.saturating_add("[REDACTED]".len());
            if search_start >= result.len() {
                break;
            }
        }
    }

    // 5. Redact environment variable assignments with sensitive suffixes
    result = ENV_SECRET_RE
        .replace_all(&result, |caps: &regex::Captures| {
            let var_name = caps.get(1).unwrap().as_str();
            format!("{var_name}=[REDACTED]")
        })
        .into_owned();

    // 6. Redact piped secrets: echo "secret" | command
    //    The secret value inside quotes is replaced with [REDACTED]
    result = PIPED_SECRET_RE
        .replace_all(&result, |caps: &regex::Captures| {
            let full_match = caps.get(0).unwrap().as_str();
            let secret = caps.get(1).unwrap().as_str();
            full_match.replace(secret, "[REDACTED]")
        })
        .into_owned();

    // 7. Redact IPv4 addresses (server IPs, infrastructure addresses).
    //    Keeps 127.0.0.1 and 0.0.0.0 as they are non-sensitive.
    result = IPV4_RE
        .replace_all(&result, |caps: &regex::Captures| {
            let ip = caps.get(1).unwrap().as_str();
            if ip == "127.0.0.1" || ip == "0.0.0.0" {
                ip.to_string()
            } else {
                "[IP_REDACTED]".to_string()
            }
        })
        .into_owned();

    result
}

/// Find a case-insensitive ASCII substring in `haystack`, returning the byte
/// position of the first match. Returns None if not found.
///
/// Unlike haystack.to_lowercase().find(), this never expands the haystack,
/// so returned positions are always valid indices in the original string.
fn find_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    debug_assert!(needle.is_ascii());
    if needle.is_empty() {
        return Some(0);
    }
    let first = needle.as_bytes()[0];
    let rest = &needle[1..];
    for (pos, chunk) in haystack.as_bytes().windows(needle.len()).enumerate() {
        if chunk[0].eq_ignore_ascii_case(&first)
            && chunk[1..]
                .iter()
                .enumerate()
                .all(|(i, &b)| b.eq_ignore_ascii_case(&rest.as_bytes()[i]))
        {
            return Some(pos);
        }
    }
    None
}

/// Recursively replace the user's home directory path with `~` in all strings.
///
/// Transforms `/Users/adolfousierstudio/srv/rs/opencrabs` → `~/srv/rs/opencrabs`
/// This makes tool call displays much cleaner in the TUI and channels.
fn shrink_home_paths(value: &Value) -> Value {
    // Cross-platform home directory detection:
    // - macOS/Linux: HOME
    // - Windows: USERPROFILE (or HOMEDRIVE + HOMEPATH as fallback)
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .or_else(|_| {
            let drive = std::env::var("HOMEDRIVE").unwrap_or_default();
            let path = std::env::var("HOMEPATH").unwrap_or_default();
            let combined = format!("{}{}", drive, path);
            if combined.is_empty() {
                Err(std::env::VarError::NotPresent)
            } else {
                Ok(combined)
            }
        });

    let home = match home {
        Ok(h) if !h.is_empty() => h,
        _ => return value.clone(),
    };

    shrink_home_paths_inner(value, &home)
}

fn shrink_home_paths_inner(value: &Value, home: &str) -> Value {
    match value {
        Value::String(s) => {
            // Replace home path at the start of the string, or anywhere inside it
            let shortened = s.replace(home, "~");
            Value::String(shortened)
        }
        Value::Object(map) => {
            let mut out = Map::with_capacity(map.len());
            for (k, v) in map {
                out.insert(k.clone(), shrink_home_paths_inner(v, home));
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(
            arr.iter()
                .map(|v| shrink_home_paths_inner(v, home))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Recursively redact sensitive fields from a tool input JSON value.
///
/// - Object keys matching `SENSITIVE_KEYS` have their string values replaced
///   with `"[REDACTED]"`
/// - The `command` field (bash) has inline secret patterns redacted
/// - The `headers` object has all values for sensitive header names redacted
/// - Arrays and nested objects are recursively processed
/// - Home directory paths are shortened to `~` for cleaner display
pub fn redact_tool_input(value: &Value) -> Value {
    // First shrink home paths, then redact secrets
    let shortened = shrink_home_paths(value);
    redact_value(&shortened, None)
}

fn redact_value(value: &Value, parent_key: Option<&str>) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = Map::with_capacity(map.len());
            for (k, v) in map {
                let redacted = if is_sensitive_key(k) {
                    // Redact the value regardless of type
                    Value::String("[REDACTED]".to_string())
                } else if k == "command" {
                    // Bash command: apply inline pattern redaction
                    match v.as_str() {
                        Some(cmd) => Value::String(redact_command(cmd)),
                        None => redact_value(v, Some(k)),
                    }
                } else if k == "headers" {
                    // Headers object: redact values for sensitive header names
                    redact_headers_object(v)
                } else if k == "query" || k == "params" {
                    // Query params object: redact sensitive param values
                    redact_value(v, Some(k))
                } else if k == "url" {
                    // URLs may have passwords embedded
                    match v.as_str() {
                        Some(url) => Value::String(redact_command(url)),
                        None => redact_value(v, Some(k)),
                    }
                } else {
                    redact_value(v, Some(k))
                };
                out.insert(k.clone(), redacted);
            }
            Value::Object(out)
        }
        Value::Array(arr) => {
            // If parent key is sensitive, redact the whole array
            if parent_key.map(is_sensitive_key).unwrap_or(false) {
                Value::String("[REDACTED]".to_string())
            } else {
                Value::Array(arr.iter().map(|v| redact_value(v, None)).collect())
            }
        }
        Value::String(s) => {
            if parent_key.map(is_sensitive_key).unwrap_or(false) {
                Value::String("[REDACTED]".to_string())
            } else {
                Value::String(s.clone())
            }
        }
        other => other.clone(),
    }
}

// ---------------------------------------------------------------------------
// Free-text secret redaction (for thinking streams, responses, intermediate text)
// ---------------------------------------------------------------------------

/// Known API key prefixes with their minimum total length.
/// Format: (prefix, min_suffix_len) — token must have at least this many chars
/// after the prefix to be considered a real key (avoids false positives on
/// short strings that happen to start with a prefix).
///
/// When adding new prefixes: prefer longer, more specific prefixes first
/// (e.g. `sk-proj-` before `sk-`). The scanner checks all prefixes, so order
/// only affects which prefix is shown in `[prefix][REDACTED]` output.
const KEY_PREFIXES: &[(&str, usize)] = &[
    // --- AI / LLM providers ---
    ("sk-proj-", 20),      // OpenAI project keys
    ("sk-ant-api03-", 20), // Anthropic API keys (full prefix)
    ("sk-ant-", 20),       // Anthropic API keys (short prefix)
    ("sk-or-v1-", 20),     // OpenRouter keys
    ("sk-cp-", 20),        // MiniMax keys
    ("sk-", 20),           // Generic OpenAI-style keys (DeepSeek, etc.)
    ("gsk_", 15),          // Groq keys
    ("nvapi-", 15),        // NVIDIA NIM keys
    ("AIzaSy", 20),        // Google AI / Firebase keys
    ("ya29.", 20),         // Google OAuth access tokens
    ("pplx-", 20),         // Perplexity keys
    ("hf_", 15),           // HuggingFace tokens
    ("r8_", 20),           // Replicate tokens
    // --- Cloud providers ---
    ("AKIA", 12), // AWS access key IDs (exactly 20 chars total)
    ("ASIA", 12), // AWS STS temporary credentials
    ("ABIA", 12), // AWS STS (another variant)
    ("ACCA", 12), // AWS CloudFront
    // --- Azure ---
    // Azure keys are 32-char base64 but headers are caught by SENSITIVE_KEYS.
    // Connection strings have a known prefix:
    ("DefaultEndpointsProtocol=", 10), // Azure Storage connection strings
    // --- Payments ---
    ("sk_live_", 15), // Stripe secret keys (live)
    ("sk_test_", 15), // Stripe secret keys (test)
    ("pk_live_", 15), // Stripe publishable keys (live)
    ("pk_test_", 15), // Stripe publishable keys (test)
    ("rk_live_", 15), // Stripe restricted keys (live)
    ("rk_test_", 15), // Stripe restricted keys (test)
    ("sq0atp-", 15),  // Square access tokens
    ("sq0csp-", 15),  // Square application secrets
    // --- Git / DevOps ---
    ("ghp_", 15),            // GitHub personal tokens
    ("gho_", 15),            // GitHub OAuth tokens
    ("ghu_", 15),            // GitHub user-to-server tokens
    ("ghs_", 15),            // GitHub server-to-server tokens
    ("github_pat_", 15),     // GitHub fine-grained tokens
    ("glpat-", 15),          // GitLab personal tokens
    ("gloas-", 15),          // GitLab OAuth tokens
    ("npm_", 15),            // npm registry tokens
    ("pypi-AgEIcHlwaS", 10), // PyPI tokens
    // --- Communication ---
    ("xoxb-", 15),    // Slack bot tokens
    ("xoxp-", 15),    // Slack user tokens
    ("xapp-", 15),    // Slack app tokens
    ("xoxs-", 15),    // Slack session tokens
    ("SG.", 20),      // SendGrid API keys
    ("xkeysib-", 15), // Brevo (Sendinblue) keys
    // --- E-commerce / SaaS ---
    ("shpat_", 15),   // Shopify admin tokens
    ("shpca_", 15),   // Shopify custom app tokens
    ("shppa_", 15),   // Shopify partner tokens
    ("shpss_", 15),   // Shopify shared secret
    ("ntn_", 15),     // Notion API tokens
    ("lin_api_", 15), // Linear API keys
    ("aio_", 15),     // Airtable tokens
    ("phc_", 15),     // PostHog keys
    // --- Infrastructure / Monitoring ---
    ("sntrys_", 15),         // Sentry auth tokens
    ("dop_v1_", 15),         // DigitalOcean tokens
    ("tskey-", 15),          // Tailscale keys
    ("tvly-", 15),           // Tavily web search keys
    ("hvs.", 15),            // HashiCorp Vault service tokens
    ("vault:v1:", 10),       // HashiCorp Vault transit-encrypted
    ("AGE-SECRET-KEY-", 10), // age encryption keys
    // --- Social / Meta ---
    ("EAA", 30),  // Facebook/Meta long-lived tokens
    ("ATTA", 30), // Trello tokens
    // --- Auth / Crypto ---
    ("eyJ", 30),    // JWT tokens (base64 of `{"`)
    ("whsec_", 15), // Webhook signing secrets
];

/// Regex for long hex strings that look like API tokens (32+ hex chars).
/// Lowered from 40 to 32 to catch Azure keys, custom service tokens, etc.
/// UUIDs are safe — they contain dashes so won't match contiguous hex.
static HEX_TOKEN_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b[0-9a-fA-F]{32,}\b").unwrap());

/// Regex for mixed alphanumeric tokens (28+ chars containing both letters and digits).
/// Catches opaque tokens like custom service keys that have no prefix.
/// Won't match pure words, pure numbers, or structured strings with separators.
static MIXED_ALNUM_TOKEN_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b[a-zA-Z0-9]{28,}\b").unwrap());

/// Regex for environment variable assignments with sensitive suffixes.
/// Matches UPPERCASE_VAR_NAME="value" or UPPERCASE_VAR_NAME=value where the
/// name ends in _PASS, _SECRET, _TOKEN, _KEY, _APIKEY, _API_KEY, _CREDENTIAL,
/// _CREDENTIALS, or _AUTH (case-insensitive suffix, uppercase name required).
/// Only matches names starting with uppercase to avoid catching lowercase
/// field names like auth_token that have dedicated prefix handlers.
static ENV_SECRET_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"\b([A-Z][A-Z0-9_]*(?i:_PASS|_PASSWORD|_PASSWD|_SECRET|_TOKEN|_KEY|_APIKEY|_API_KEY|_CREDENTIAL|_CREDENTIALS|_AUTH))\s*=\s*(?:")?([^\s"']+)"#).unwrap()
});

/// Regex for piped secrets: echo "secret" | command or echo 'secret' | command
/// Catches patterns where a secret is piped to docker login, kubectl, etc.
static PIPED_SECRET_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?i)echo\s+["']([a-zA-Z0-9_\-+/=]{20,})["']\s*\|"#).unwrap());

/// Regex for IPv4 addresses: four octets of 1-3 digits separated by dots.
/// Uses word boundaries to avoid matching inside longer strings.
/// Excludes common version-like patterns (e.g. `0.0.0.0`, `1.0.0`, `v1.2.3`).
static IPV4_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})\b").unwrap());

/// Regex for Qwen 3 / DeepSeek SentencePiece-style tool-call markers.
/// Matches the full token family: `<|tool<sep>calls_section_begin|>`,
/// `<|tool<sep>call_begin|>`, `<|tool<sep>call_end|>`,
/// `<|tool<sep>calls_section_end|>`, `<|tool<sep>call_argument_begin|>`,
/// where `<sep>` is either U+2581 (`▁`, the real SentencePiece word-
/// boundary char emitted by Qwen 3 vLLM endpoints) or an ASCII `_`
/// (emitted by some quantizations / forks).
///
/// Why a catch-all rather than enumerating each marker: when Qwen 3
/// degrades into repetition mode it can emit unknown variants like
/// `<|tool▁call_metadata|>` or future tokens we haven't seen yet. The
/// `[^|]*` middle drops anything that fits the bracket shape, so the
/// regex stays correct as the format grows. The leading `<|tool` plus
/// the U+2581-or-underscore separator is specific enough that prose
/// like `<|tool of choice|>` will not match (no separator follows).
static QWEN_TOOL_MARKER_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"<\|tool[\u{2581}_][^|]*\|>").unwrap());

/// Redact API keys and tokens from free-form text (thinking, responses, etc.).
///
/// Catches:
/// - Known provider key prefixes (`sk-proj-...`, `xoxb-...`, `AIzaSy...`, etc.)
/// - Long hex token strings (32+ chars — Azure keys, custom tokens, etc.)
/// - Mixed alphanumeric tokens (28+ chars with both letters and digits)
/// - Bearer/Authorization inline patterns
///
/// Preserves the prefix so the user can see *what kind* of key was redacted.
pub fn redact_secrets(text: &str) -> String {
    let mut result = text.to_string();

    // 1. Redact known key prefixes — keep prefix, replace rest with [REDACTED]
    // All KEY_PREFIXES are ASCII, so we use find_case_insensitive to avoid
    // to_lowercase() Unicode-expansion causing byte-offset misalignment.
    for &(prefix, min_suffix_len) in KEY_PREFIXES {
        let mut search_from = 0;
        while let Some(abs_pos) = find_case_insensitive(&result[search_from..], prefix) {
            let true_pos = search_from + abs_pos;
            let after = true_pos + prefix.len();
            // Guard: after can exceed result.len() when Unicode chars in the
            // prefix region expanded on to_lowercase(). find_case_insensitive
            // avoids this, but we keep the guard as defence-in-depth.
            if after > result.len() {
                break;
            }
            // Find end of token: next whitespace, quote, comma, backtick, or end
            let end = result[after..]
                .find(|c: char| {
                    c.is_whitespace()
                        || matches!(
                            c,
                            '"' | '\'' | ',' | '`' | ')' | ']' | '}' | '>' | '|' | ';'
                        )
                })
                .map(|p| after + p)
                .unwrap_or(result.len());
            let suffix_len = end - after;
            if suffix_len >= min_suffix_len {
                // Keep prefix visible, redact the rest
                result.replace_range(after..end, "[REDACTED]");
            }
            // Advance past the replacement (or, when we didn't replace,
            // past `prefix.len()` worth of bytes so we don't re-scan
            // the same prefix forever). MUST snap to a UTF-8 char
            // boundary — `after + "[REDACTED]".len()` is a byte
            // arithmetic operation and can land inside a multi-byte
            // character (e.g. `→` U+2192 is 3 bytes, so `after + 10`
            // can stop at byte 2 of 3 inside it). The next iteration
            // would then panic at the slice on line 461 with
            // `start byte index N is not a char boundary; it is inside
            // '→' (bytes M..M+3)`. Repro: Ctrl+O expand a thinking
            // block that contains `→` after a prefix-shaped string.
            let raw_next = after.saturating_add("[REDACTED]".len());
            search_from = if raw_next >= result.len() {
                result.len()
            } else {
                // Snap forward to a valid char boundary. `ceil_char_boundary`
                // is stable since Rust 1.79.
                result.ceil_char_boundary(raw_next)
            };
            if search_from >= result.len() {
                break;
            }
        }
    }

    // Helper: a 28+ char alphanumeric blob preceded by `/` is almost always
    // a URL path segment, not a secret. Google Drive `/file/d/<33-char-id>`,
    // Docs `/document/d/<id>`, YouTube `/watch?v=<id>`, GitHub
    // `/commit/<40-char-sha>`, etc. — all public identifiers that users
    // need to click. 2026-04-18 22:29 screenshot: a Drive file ID got
    // redacted to `[REDACTED_TOKEN]`, breaking the link. Real secrets
    // appear after `=`, `:`, or whitespace, never after `/`.
    let is_url_path_segment =
        |input: &str, match_start: usize| -> bool { input[..match_start].ends_with('/') };

    // 2. Redact long hex tokens (32+ chars — catches Azure keys, custom service tokens, etc.)
    result = HEX_TOKEN_RE
        .replace_all(&result, |caps: &regex::Captures| {
            let m = caps.get(0).unwrap();
            let token = m.as_str();
            if is_url_path_segment(&result, m.start()) {
                token.to_string()
            } else {
                "[REDACTED_TOKEN]".to_string()
            }
        })
        .into_owned();

    // 3. Redact mixed alphanumeric tokens (28+ chars with both letters AND digits).
    //    Catches opaque keys like custom service tokens with no prefix.
    //    Only redacts if the match contains both digits and letters (avoids
    //    false positives on long words like "acknowledgement" or pure numbers).
    result = MIXED_ALNUM_TOKEN_RE
        .replace_all(&result, |caps: &regex::Captures| {
            let m = caps.get(0).unwrap();
            let token = m.as_str();
            let has_digit = token.chars().any(|c| c.is_ascii_digit());
            let has_alpha = token.chars().any(|c| c.is_ascii_alphabetic());
            if has_digit && has_alpha && !is_url_path_segment(&result, m.start()) {
                "[REDACTED_TOKEN]".to_string()
            } else {
                token.to_string()
            }
        })
        .into_owned();

    // 4. Redact inline "Bearer <token>" patterns (ASCII-only, same fix as redact_command)
    for pattern in &["bearer ", "authorization: bearer "] {
        let mut search_start = 0;
        while let Some(abs_pos) = find_case_insensitive(&result[search_start..], pattern) {
            let true_pos = search_start + abs_pos;
            let after = true_pos + pattern.len();
            // Bounds check: after can exceed result.len() when earlier chars
            // expanded on to_lowercase() — same Unicode issue as in redact_command.
            if after > result.len() {
                break;
            }
            let end = result[after..]
                .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '`' | ')'))
                .map(|p| after + p)
                .unwrap_or(result.len());
            if end > after {
                result.replace_range(after..end, "[REDACTED]");
            }
            search_start = after.saturating_add("[REDACTED]".len());
            if search_start >= result.len() {
                break;
            }
        }
    }

    // 5. Redact environment variable assignments with sensitive suffixes
    result = ENV_SECRET_RE
        .replace_all(&result, |caps: &regex::Captures| {
            let var_name = caps.get(1).unwrap().as_str();
            format!("{var_name}=[REDACTED]")
        })
        .into_owned();

    // 6. Redact piped secrets: echo "secret" | command
    //    The secret value inside quotes is replaced with [REDACTED]
    result = PIPED_SECRET_RE
        .replace_all(&result, |caps: &regex::Captures| {
            let full_match = caps.get(0).unwrap().as_str();
            let secret = caps.get(1).unwrap().as_str();
            full_match.replace(secret, "[REDACTED]")
        })
        .into_owned();

    // 7. Redact IPv4 addresses (server IPs, infrastructure addresses).
    //    Keeps 127.0.0.1 and 0.0.0.0 as they are non-sensitive.
    result = IPV4_RE
        .replace_all(&result, |caps: &regex::Captures| {
            let ip = caps.get(1).unwrap().as_str();
            if ip == "127.0.0.1" || ip == "0.0.0.0" {
                ip.to_string()
            } else {
                "[IP_REDACTED]".to_string()
            }
        })
        .into_owned();

    result
}

/// Strip LLM-hallucinated artifacts from text before external delivery.
///
/// Removes:
/// - HTML comments (`<!-- tools-v2: ... -->`, `<!-- lens -->`, etc.)
/// - XML tool-call blocks (`<tool_call>`, `<tool_code>`, `<minimax:tool_call>`, etc.)
/// - Cursor / Aider / Cline-style `CODE_EDIT_BLOCK` fenced blocks that
///   some fine-tuned models (qwen-3.7-max-thinking observed 2026-05-30
///   14:13) emit as text instead of calling `edit_file`. They look
///   like ```` ```lang|CODE_EDIT_BLOCK|/abs/path/to/file ```` and leak
///   the full file contents to whichever channel renders the
///   response.
///
/// Use this on any text sent to Telegram, HTTP webhooks, or other external channels.
pub fn strip_llm_artifacts(text: &str) -> String {
    use crate::brain::agent::service::AgentService;

    let mut result = text.to_string();
    // Qwen 3 / DeepSeek SentencePiece-style tool-call tokens.
    // The streaming filter in custom_openai_compatible.rs catches
    // these in real time, but a chunk-boundary near a marker can
    // briefly leak one through, and the format may grow new
    // variants the streaming filter doesn't know yet. This regex
    // sweeps anything matching `<|tool<sep>...|>` where <sep> is
    // either U+2581 (the real SP word-boundary char Qwen emits) or
    // an ASCII underscore (emitted by some quantizations). Matches
    // the open/close tag family in one pass: section_begin,
    // section_end, call_begin, call_end, call_argument_begin, etc.
    if result.contains("<|tool") {
        result = QWEN_TOOL_MARKER_RE.replace_all(&result, "").into_owned();
    }
    if result.contains("<!--") {
        result = AgentService::strip_html_comments(&result);
    }
    if result.contains("CODE_EDIT_BLOCK") {
        result = strip_code_edit_block_fences(&result);
    }
    // Two strip paths:
    //   1. Matched-pair JSON-bearing tool-call blocks — only stripped
    //      when parse_xml_tool_calls confirms there's real call JSON
    //      inside, so prose like "we fixed the <tool_call> bug" survives.
    //   2. Orphan close tags (`</tool_result>`, `</tool_call>`, etc.) —
    //      always stripped because models routinely emit them alone when
    //      the opener was eaten by an earlier pass or never produced.
    //      2026-05-28 user report: `</tool_result>` rendered visibly
    //      between paragraphs in the TUI.
    if AgentService::has_xml_tool_block(&result) {
        let parsed = AgentService::parse_xml_tool_calls(&result);
        if !parsed.is_empty() {
            result = AgentService::strip_xml_tool_calls(&result);
        }
    }
    // Orphan-close pass runs unconditionally — it's safe because it
    // only matches close tags that are alone on their line (with
    // optional surrounding whitespace).
    if result.contains("</tool_result>")
        || result.contains("</tool_call>")
        || result.contains("</tool_use>")
        || result.contains("</invoke>")
        || result.contains("</function_calls>")
        || result.contains("</qwen:tool_call>")
        || result.contains("</minimax:tool_call>")
    {
        result = AgentService::strip_xml_tool_calls(&result);
    }
    result
}

/// Strip `CODE_EDIT_BLOCK`-style fenced blocks the LLM emits as text
/// when it confuses a Cursor/Aider/Cline IDE edit format with an
/// actual tool call.
///
/// Pattern:
/// ````text
/// ```<language>|CODE_EDIT_BLOCK|<absolute path>
/// <file contents -- often hundreds of lines>
/// ```
/// ````
///
/// Replace the whole block (header line + body + closing fence) with
/// a single short notice so the user knows the agent TRIED to edit a
/// file but used the wrong format — and so the chat doesn't leak the
/// full file contents through Telegram / Slack / Discord etc. The
/// notice mentions the path so the user can verify nothing weird was
/// attempted.
///
/// Why not silently drop the path too: the path is the one piece of
/// signal worth keeping (the user can audit what file the agent
/// thought it was touching). The body is just regurgitated file
/// content with zero new information.
pub(crate) fn strip_code_edit_block_fences(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if is_code_edit_block_open(line) {
            let path = extract_code_edit_block_path(line).unwrap_or("(unknown)");
            // Skip ahead to the closing ``` fence. If the model
            // forgot the closer (truncated stream) we still drop the
            // rest of the input so partial file contents don't leak.
            let mut j = i + 1;
            while j < lines.len() && !lines[j].trim_start().starts_with("```") {
                j += 1;
            }
            out.push(format!(
                "[Agent attempted to edit `{path}` using an unsupported \
                 inline-edit format. The change was NOT applied — agent \
                 should retry via the `edit_file` tool.]"
            ));
            // j points at the closing ```; skip past it (or to EOF
            // if the closer was missing).
            i = j.saturating_add(1);
            continue;
        }
        out.push(line.to_string());
        i += 1;
    }
    out.join("\n")
}

/// True for a line that opens a `CODE_EDIT_BLOCK` fenced block.
/// Tolerant of leading whitespace and unknown language tags — the
/// `CODE_EDIT_BLOCK` marker is the load-bearing piece.
fn is_code_edit_block_open(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("```") && trimmed.contains("CODE_EDIT_BLOCK")
}

/// Extract the path component from a `CODE_EDIT_BLOCK` opener line.
/// Format: ` ```<lang>|CODE_EDIT_BLOCK|<path>`. Returns None when
/// the path slot is missing.
fn extract_code_edit_block_path(line: &str) -> Option<&str> {
    let after = line.split("CODE_EDIT_BLOCK").nth(1)?;
    let path = after.trim_start_matches('|').trim();
    if path.is_empty() { None } else { Some(path) }
}

/// Redact values inside a headers object for known sensitive header names.
fn redact_headers_object(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = Map::with_capacity(map.len());
            for (k, v) in map {
                let redacted = if is_sensitive_key(k) {
                    Value::String("[REDACTED]".to_string())
                } else {
                    v.clone()
                };
                out.insert(k.clone(), redacted);
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redacts_authorization_header() {
        let input = json!({
            "method": "POST",
            "url": "https://api.trello.com/1/cards",
            "headers": {
                "Authorization": "Bearer sk-trello-abc123",
                "Content-Type": "application/json"
            }
        });
        let out = redact_tool_input(&input);
        assert_eq!(out["headers"]["Authorization"], "[REDACTED]");
        assert_eq!(out["headers"]["Content-Type"], "application/json");
    }

    #[test]
    fn redacts_api_key_field() {
        let input = json!({"api_key": "secret123", "query": "something"});
        let out = redact_tool_input(&input);
        assert_eq!(out["api_key"], "[REDACTED]");
        assert_eq!(out["query"], "something");
    }

    #[test]
    fn redacts_bash_bearer_token() {
        let input = json!({
            "command": "curl -H \"Authorization: Bearer sk-abc123\" https://api.example.com"
        });
        let out = redact_tool_input(&input);
        let cmd = out["command"].as_str().unwrap();
        assert!(cmd.contains("[REDACTED]"), "expected REDACTED in: {cmd}");
        assert!(!cmd.contains("sk-abc123"), "secret still present: {cmd}");
    }

    #[test]
    fn redacts_url_password() {
        let input = json!({
            "url": "https://user:mysecretpass@api.example.com/v1"
        });
        let out = redact_tool_input(&input);
        let url = out["url"].as_str().unwrap();
        assert!(url.contains("[REDACTED]"), "expected REDACTED in: {url}");
        assert!(
            !url.contains("mysecretpass"),
            "password still present: {url}"
        );
    }

    #[test]
    fn preserves_non_sensitive_fields() {
        let input = json!({
            "method": "GET",
            "url": "https://api.example.com/data",
            "timeout_secs": 30
        });
        let out = redact_tool_input(&input);
        assert_eq!(out["method"], "GET");
        assert_eq!(out["timeout_secs"], 30);
    }

    // --- redact_secrets (free-text) tests ---

    #[test]
    fn redact_secrets_openai_key() {
        let text =
            "The API key is sk-proj-mrRb3y9swLqHv8ZzB9lPH0_V7RPruzdbnXJf34DxU2RCdQnhCYjS99Tj ok?";
        let out = redact_secrets(text);
        assert!(out.contains("sk-proj-[REDACTED]"), "got: {out}");
        assert!(!out.contains("mrRb3y"), "secret leaked: {out}");
        assert!(out.contains("ok?"), "trailing text lost: {out}");
    }

    #[test]
    fn redact_secrets_anthropic_key() {
        let text = "Use sk-ant-oat01-H9Uogg04aohFVZn5qymS8R for auth";
        let out = redact_secrets(text);
        assert!(out.contains("sk-ant-[REDACTED]"), "got: {out}");
        assert!(!out.contains("H9Uogg"), "secret leaked: {out}");
    }

    #[test]
    fn redact_secrets_slack_token() {
        // Construct at runtime so the literal token prefix never appears in source
        let token = String::from("xo") + "xb-" + "fake_test_token_not_real";
        let text = format!("slack token: {token}");
        let out = redact_secrets(&text);
        let expected = String::from("xo") + "xb-[REDACTED]";
        assert!(out.contains(&expected), "got: {out}");
    }

    #[test]
    fn redact_secrets_google_key() {
        let text = "key=AIzaSyFAKE_TEST_KEY_NOT_REAL_000000 for gemini";
        let out = redact_secrets(text);
        assert!(out.contains("AIzaSy[REDACTED]"), "got: {out}");
    }

    #[test]
    fn redact_secrets_hex_token() {
        let text = "auth_token=aa83802d35bb2c4471e7e96f4eaeafa6c96fe42f set";
        let out = redact_secrets(text);
        assert!(out.contains("[REDACTED_TOKEN]"), "got: {out}");
        assert!(!out.contains("aa83802d"), "secret leaked: {out}");
    }

    #[test]
    fn redact_secrets_preserves_normal_text() {
        let text = "The model is claude-3-opus and the temperature is 0.7";
        let out = redact_secrets(text);
        assert_eq!(out, text);
    }

    #[test]
    fn redact_secrets_multiple_keys() {
        let text = "OpenAI: sk-proj-AAAAAAAAAAAAAAAAAAAAAA, Groq: gsk_BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";
        let out = redact_secrets(text);
        assert!(out.contains("sk-proj-[REDACTED]"), "got: {out}");
        assert!(out.contains("gsk_[REDACTED]"), "got: {out}");
    }

    // --- New pattern tests ---

    #[test]
    fn redact_secrets_stripe_live_key() {
        let text = "stripe key: sk_live_FAKE00TEST00KEY00EXAMPLE00VAL";
        let out = redact_secrets(text);
        assert!(out.contains("sk_live_[REDACTED]"), "got: {out}");
        assert!(!out.contains("FAKE00TEST"), "secret leaked: {out}");
    }

    #[test]
    fn redact_secrets_aws_access_key() {
        let text = "AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE";
        let out = redact_secrets(text);
        assert!(out.contains("AKIA[REDACTED]"), "got: {out}");
        assert!(!out.contains("IOSFODNN"), "secret leaked: {out}");
    }

    #[test]
    fn redact_secrets_sendgrid_key() {
        let text = "SENDGRID_API_KEY=SG.abc123def456ghi789jkl012mno345pqr678stu901vwx234yz";
        let out = redact_secrets(text);
        assert!(out.contains("SENDGRID_API_KEY=[REDACTED]"), "got: {out}");
    }

    #[test]
    fn redact_secrets_jwt_token() {
        let text = "token: eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N";
        let out = redact_secrets(text);
        assert!(out.contains("eyJ[REDACTED]"), "got: {out}");
    }

    #[test]
    fn redact_secrets_mixed_alnum_opaque_token() {
        // Simulates tokens like agentverse keys — no prefix, mixed letters+digits, 32 chars
        let text = "key: 38947394723jkhkrjkhdfiuo83489732 done";
        let out = redact_secrets(text);
        assert!(
            out.contains("[REDACTED_TOKEN]"),
            "opaque mixed-alnum token not caught: {out}"
        );
        assert!(!out.contains("38947394723"), "secret leaked: {out}");
    }

    #[test]
    fn redact_secrets_hex_32_chars() {
        // 32-char hex token (e.g. Azure API key)
        let text = "api-key: a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4 end";
        let out = redact_secrets(text);
        assert!(
            out.contains("[REDACTED_TOKEN]"),
            "32-char hex not caught: {out}"
        );
    }

    #[test]
    fn redact_secrets_preserves_short_alnum() {
        // Short alphanumeric strings should NOT be redacted
        let text = "model claude3opus version 12345 session abc123";
        let out = redact_secrets(text);
        assert_eq!(out, text, "short strings should be preserved");
    }

    #[test]
    fn redact_secrets_preserves_pure_alpha_long() {
        // Long pure-alpha strings (English words) should NOT be redacted
        let text = "the acknowledgementofresponsibility was important";
        let out = redact_secrets(text);
        assert_eq!(out, text, "pure-alpha long string should be preserved");
    }

    #[test]
    fn redact_secrets_shopify_token() {
        let text = "token: shpat_abc123def456ghi789jkl012mno";
        let out = redact_secrets(text);
        assert!(out.contains("shpat_[REDACTED]"), "got: {out}");
    }

    #[test]
    fn redact_secrets_digital_ocean_token() {
        let text = "DO_TOKEN=dop_v1_abc123def456ghi789jkl012mno345";
        let out = redact_secrets(text);
        assert!(out.contains("DO_TOKEN=[REDACTED]"), "got: {out}");
    }

    // --- Unicode-expansion regression tests ---
    // These verify that to_lowercase() byte-offset mismatch does not cause panic
    // or wrong redaction when Unicode chars expand on lowercase (e.g. Turkish
    // 'İ' → 'i̇' adds a combining dot, 2→3 bytes).

    #[test]
    fn redact_command_unicode_expansion_no_panic() {
        // 'İ' expands to 'i̇' (2→3 bytes) on to_lowercase().
        // Before the fix, match_pos exceeded result.len() and panicked.
        let input = "İİİİİİİİİİauthorization: bearer sk-secret-123";
        let out = redact_command(input);
        // Must not panic, and secret must be redacted
        assert!(out.contains("[REDACTED]"), "secret not redacted: {out}");
        assert!(!out.contains("sk-secret-123"), "secret leaked: {out}");
    }

    #[test]
    fn redact_command_unicode_expansion_api_key() {
        // Same issue with api_key= prefix
        let input = "İİİİİİİİİİapi_key=super-secret-key";
        let out = redact_command(input);
        assert!(out.contains("[REDACTED]"), "secret not redacted: {out}");
        assert!(!out.contains("super-secret-key"), "secret leaked: {out}");
    }

    #[test]
    fn redact_secrets_unicode_expansion_no_panic() {
        // Unicode expansion before an sk- key prefix — same panic scenario
        let input = "İİİİİİİİİİ sk-proj-mrRb3y9swLqHv8ZzB9lPH0_V7RPruzdbnXJf34DxU2RCdQnhCYjS99Tj";
        let out = redact_secrets(input);
        assert!(out.contains("[REDACTED]"), "secret not redacted: {out}");
        assert!(!out.contains("mrRb3y"), "secret leaked: {out}");
    }

    #[test]
    fn redact_command_unicode_expansion_bearer() {
        // Unicode before "bearer " pattern
        let input = "İİİİİİİİİİ bearer eyJhbGc...";
        let out = redact_command(input);
        assert!(out.contains("[REDACTED]"), "token not redacted: {out}");
        assert!(!out.contains("eyJhbGc"), "token leaked: {out}");
    }

    #[test]
    fn redact_secrets_unicode_expansion_bearer() {
        // Bearer pattern in redact_secrets with Unicode expansion
        let input = "İİİİİİİİİİbearer eyJhbGciOiJIUzI1NiJ9.test";
        let out = redact_secrets(input);
        assert!(out.contains("[REDACTED]"), "token not redacted: {out}");
        assert!(!out.contains("eyJhbGc"), "token leaked: {out}");
    }

    #[test]
    fn redact_command_unicode_normal_text() {
        // Normal text with no secrets — should be unchanged
        let input = "Normal text with İstanbul and Größe and Ñoño";
        let out = redact_command(input);
        assert_eq!(out, input, "normal text should not change");
    }

    #[test]
    fn redact_secrets_unicode_normal_text() {
        // Normal text with no secrets — should be unchanged
        let input = "Hello world, İstanbul, München, Ñoño";
        let out = redact_secrets(input);
        assert_eq!(out, input, "normal text should not change");
    }

    // --- Home path shortening tests ---

    // Home-path shrinking tests construct inputs from `$HOME` and assert
    // the redactor collapses them to `~`. On Windows there's no `HOME`
    // by default — the redactor reads `%USERPROFILE%` (e.g.
    // `C:\Users\runneradmin`) while these tests fall back to a fake
    // `/Users/testuser` Unix path that the redactor never recognises.
    // Gating the tests to Unix keeps the contract strict on the
    // platforms where it actually applies.
    #[cfg(unix)]
    #[test]
    fn shrinks_home_path_in_string() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/testuser".to_string());
        let input = json!({"path": format!("{}/srv/rs/opencrabs", home)});
        let out = redact_tool_input(&input);
        assert_eq!(out["path"], "~/srv/rs/opencrabs");
    }

    #[cfg(unix)]
    #[test]
    fn shrinks_home_path_in_nested_object() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/testuser".to_string());
        let input = json!({
            "config": {
                "dir": format!("{}/.opencrabs", home),
                "name": "test"
            }
        });
        let out = redact_tool_input(&input);
        assert_eq!(out["config"]["dir"], "~/.opencrabs");
        assert_eq!(out["config"]["name"], "test");
    }

    #[cfg(unix)]
    #[test]
    fn shrinks_home_path_in_array() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/testuser".to_string());
        let input = json!([format!("{}/file1.rs", home), format!("{}/file2.rs", home)]);
        let out = redact_tool_input(&input);
        assert_eq!(out[0], "~/file1.rs");
        assert_eq!(out[1], "~/file2.rs");
    }

    #[cfg(unix)]
    #[test]
    fn shrinks_home_path_in_bash_command() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/testuser".to_string());
        let input = json!({"command": format!("cat {}/.opencrabs/config.toml", home)});
        let out = redact_tool_input(&input);
        assert!(
            out["command"]
                .as_str()
                .unwrap()
                .contains("~/.opencrabs/config.toml")
        );
    }

    #[test]
    fn preserves_non_home_paths() {
        let input = json!({"path": "/etc/hosts", "other": "/var/log/syslog"});
        let out = redact_tool_input(&input);
        assert_eq!(out["path"], "/etc/hosts");
        assert_eq!(out["other"], "/var/log/syslog");
    }

    #[cfg(unix)]
    #[test]
    fn shrinks_home_path_mid_string() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/testuser".to_string());
        let input = json!({"msg": format!("Found at {}/docs/readme.md", home)});
        let out = redact_tool_input(&input);
        assert_eq!(out["msg"], "Found at ~/docs/readme.md");
    }
}
