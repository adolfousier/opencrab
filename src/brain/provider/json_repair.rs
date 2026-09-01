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

// ── Call-shaped JSON detection (tool-text leak defense) ─────────────────
// fork #66, ex-upstream adolfousier/opencrabs#1260.
//
// When a model with weak function-calling support "invokes" a tool, it
// dumps the call as raw JSON text instead of a structured tool_calls
// field. The rescue layer (extract_text_tool_calls) converts known shapes;
// what SURVIVES rescue is unparseable or unknown-shape residue that used
// to ride to the user as the final answer.
//
// The detector here replaces the old keyword heuristic ("contains
// \"function\" && contains \"arguments\"") which false-positived on prose
// ABOUT tool calls. Rules:
//   1. Only a PARSEABLE JSON object counts (keyword hits on broken JSON in
//      prose no longer fire the leak path).
//   2. The object must be call-shaped: name+arguments, bare command, or
//      OpenAI-legacy function{name,arguments}.
//   3. Fenced ```json code blocks are EXCLUDED — they are display
//      artifacts of discussion (agents routinely explain tool-call JSON
//      in fenced examples). The rescue layer above still converts known
//      fenced shapes to real calls before detection runs.
//   4. Unfenced parseable call-shaped objects are treated as genuine
//      leaked invocations: stripped from content, flagged for retry.

use super::types::ContentBlock;

/// Byte ranges of fenced code blocks (``` ... ```) in `text`.
/// An unterminated fence swallows the rest of the text.
fn fenced_code_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut in_fence = false;
    let mut fence_start = 0usize;
    let mut offset = 0usize;
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            if in_fence {
                ranges.push((fence_start, offset + line.len()));
                in_fence = false;
            } else {
                in_fence = true;
                fence_start = offset;
            }
        }
        offset += line.len();
    }
    if in_fence {
        ranges.push((fence_start, offset));
    }
    ranges
}

/// Does `candidate` parse as a JSON object with tool-invocation shape?
fn is_call_shaped_json(candidate: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(candidate) else {
        return false;
    };
    let Some(obj) = v.as_object() else {
        return false;
    };
    // Shape 1: {"name": "...", "arguments": {...}}
    if obj.get("name").is_some_and(|n| n.is_string()) && obj.contains_key("arguments") {
        return true;
    }
    // Shape 2: bare execution object {"command": ...}
    if obj.contains_key("command") {
        return true;
    }
    // Shape 3: OpenAI legacy {"function": {"name": ..., "arguments": ...}}
    if let Some(f) = obj.get("function")
        && f.get("name").is_some_and(|n| n.is_string())
        && f.get("arguments").is_some()
    {
        return true;
    }
    false
}

/// Find spans of UNFENCED, parseable, call-shaped JSON objects in `text`.
/// Outermost matching objects are returned; their nested content is not
/// re-scanned (an outer span covers its inner keys).
pub fn find_call_shaped_json_spans(text: &str) -> Vec<(usize, usize)> {
    let fenced = fenced_code_ranges(text);
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            // String-aware scan to the matching close brace.
            let mut depth = 0i32;
            let mut in_string = false;
            let mut escape = false;
            let mut j = i;
            let mut closed = false;
            while j < bytes.len() {
                let b = bytes[j];
                if in_string {
                    if escape {
                        escape = false;
                    } else if b == b'\\' {
                        escape = true;
                    } else if b == b'"' {
                        in_string = false;
                    }
                } else {
                    match b {
                        b'"' => in_string = true,
                        b'{' => depth += 1,
                        b'}' => {
                            depth -= 1;
                            if depth == 0 {
                                closed = true;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                j += 1;
            }
            if closed {
                let end = j + 1;
                let in_fence = fenced.iter().any(|(s, e)| i >= *s && j < *e);
                if !in_fence && is_call_shaped_json(&text[i..end]) {
                    spans.push((i, end));
                    i = end; // skip past the matched object
                    continue;
                }
            }
            // Unbalanced or not call-shaped: step forward so nested
            // objects still get scanned.
        }
        i += 1;
    }
    spans
}

/// Remove call-shaped JSON spans from `text`. Returns the cleaned text and
/// whether anything was stripped. Overlaps (nested matches inside an
/// already-removed outer span) are consumed by the outermost span.
pub fn strip_call_shaped_json(text: &str) -> (String, bool) {
    let mut spans = find_call_shaped_json_spans(text);
    if spans.is_empty() {
        return (text.to_string(), false);
    }
    spans.sort_unstable();
    let mut out = String::with_capacity(text.len());
    let mut last_end = 0usize;
    for (s, e) in spans {
        if s < last_end {
            continue;
        }
        out.push_str(&text[last_end..s]);
        last_end = e;
    }
    out.push_str(&text[last_end..]);
    (out.trim().to_string(), true)
}

/// Leak predicate over final content blocks: true when the response has NO
/// structured ToolUse blocks but its text contains unfenced call-shaped
/// JSON — i.e. the model tried to invoke tools as text and rescue failed.
pub fn content_has_unrecovered_tool_text(blocks: &[ContentBlock]) -> bool {
    if blocks
        .iter()
        .any(|b| matches!(b, ContentBlock::ToolUse { .. }))
    {
        return false;
    }
    blocks.iter().any(|b| match b {
        ContentBlock::Text { text } => !find_call_shaped_json_spans(text).is_empty(),
        _ => false,
    })
}
