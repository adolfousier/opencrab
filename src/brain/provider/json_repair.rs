//! Partial JSON repair for truncated tool-call arguments.
//!
//! When a streaming response gets cut mid-tool-call (network drop, timeout,
//! provider crash), the accumulator holds a partial JSON string like
//! `{"command":"git status` or `{"path":"/foo","content":"hello wo`.
//! Standard `serde_json::from_str` rejects these and the entire tool call is
//! lost. This module attempts a best-effort repair so the partial intent
//! survives:
//!
//! 1. **Close open string**: trailing unmatched `"` → close it.
//! 2. **Balance brackets**: count unclosed `{`/`[`, append matching `}`/`]`.
//! 3. **Strip trailing comma** before close.
//! 4. **Drop trailing key without value**: `{"a":1,"b":` → `{"a":1}`.
//!
//! On success returns the parsed JSON. On failure returns
//! `Some({"_partial": "<original>", "_repair_failed": true})` so the tool
//! invocation can still surface the truncated args (via tool error) instead
//! of silently dropping the call.

use serde_json::Value;

/// Try parsing as-is, then attempt repair, then fall back to a partial
/// envelope. Always returns Some — never silently drops.
pub fn parse_or_repair(raw: &str) -> Value {
    if raw.trim().is_empty() {
        return serde_json::json!({});
    }
    if let Ok(v) = serde_json::from_str::<Value>(raw) {
        return v;
    }
    if let Some(repaired) = try_repair(raw)
        && let Ok(v) = serde_json::from_str::<Value>(&repaired)
    {
        tracing::warn!(
            "[JSON_REPAIR] recovered partial args ({} bytes → {} bytes): {:?}",
            raw.len(),
            repaired.len(),
            raw.chars()
                .rev()
                .take(80)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>()
        );
        return v;
    }
    // Surface truncation explicitly so the tool dispatch can show a useful
    // error rather than silently swallowing the call.
    tracing::warn!(
        "[JSON_REPAIR] FAILED to recover partial args ({} bytes): {:?}",
        raw.len(),
        raw.chars().take(200).collect::<String>()
    );
    serde_json::json!({
        "_partial": raw,
        "_repair_failed": true,
    })
}

/// Attempt to close open strings and balance brackets so the result parses
/// as valid JSON. Returns None when the input is too broken (unbalanced
/// quotes inside a key name, malformed escapes, etc.).
pub fn try_repair(raw: &str) -> Option<String> {
    let mut chars: Vec<char> = raw.chars().collect();
    // Drop trailing whitespace
    while chars.last().is_some_and(|c| c.is_whitespace()) {
        chars.pop();
    }
    if chars.is_empty() {
        return None;
    }

    // Walk the string tracking quote/escape state and bracket depth.
    let mut in_string = false;
    let mut escape = false;
    let mut stack: Vec<char> = Vec::new();
    // Track byte positions of quotes so we can detect "key without value".
    let mut last_complete_value_end: Option<usize> = None;
    let mut after_colon = false;

    for (i, &c) in chars.iter().enumerate() {
        if escape {
            escape = false;
            continue;
        }
        if c == '\\' && in_string {
            escape = true;
            continue;
        }
        if c == '"' {
            in_string = !in_string;
            if !in_string {
                last_complete_value_end = Some(i);
                after_colon = false; // we just closed a string value
            }
            continue;
        }
        if in_string {
            continue;
        }
        match c {
            '{' | '[' => {
                stack.push(c);
                after_colon = false; // value started
            }
            '}' => {
                if stack.last() == Some(&'{') {
                    stack.pop();
                    last_complete_value_end = Some(i);
                    after_colon = false;
                } else {
                    return None; // mismatched
                }
            }
            ']' => {
                if stack.last() == Some(&'[') {
                    stack.pop();
                    last_complete_value_end = Some(i);
                    after_colon = false;
                } else {
                    return None;
                }
            }
            ':' => after_colon = true,
            ',' => after_colon = false,
            c if !c.is_whitespace() => {
                // Numbers, true/false/null — anything that's a primitive value
                last_complete_value_end = Some(i);
                after_colon = false;
            }
            _ => {}
        }
    }

    let mut out: String = chars.iter().collect();

    // Close an unterminated string.
    if in_string {
        out.push('"');
    }

    // If we ended right after a `:` with no value, drop the trailing key.
    // e.g. `{"a":1,"b":` → `{"a":1}`. Safer than appending `null`.
    if after_colon
        && !in_string
        && let Some(end) = last_complete_value_end
    {
        // Find the comma or `{` before the trailing key
        let bytes = out.as_bytes();
        // Look back from `end` for `,` or `{`
        let mut cut = None;
        for (i, &b) in bytes.iter().enumerate().take(end + 1).rev() {
            if b == b',' {
                cut = Some(i);
                break;
            }
            if b == b'{' {
                cut = Some(i + 1);
                break;
            }
        }
        if let Some(c) = cut {
            out.truncate(c);
            // If we cut at a `,`, drop the comma too so we don't leave `{"a":1,}`
            if out.ends_with(',') {
                out.pop();
            }
        }
    }

    // Strip trailing comma so `{"a":1,` → `{"a":1`
    let trimmed = out.trim_end();
    if let Some(stripped) = trimmed.strip_suffix(',') {
        out = stripped.to_string();
    }

    // Close any unclosed brackets in reverse order.
    while let Some(open) = stack.pop() {
        match open {
            '{' => out.push('}'),
            '[' => out.push(']'),
            _ => {}
        }
    }

    Some(out)
}
