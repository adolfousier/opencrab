//! Tests for the context-manifest trace hook (#620).

use crate::brain::provider::{LLMRequest, Message, Tool};
use crate::eval::manifest::ContextManifest;

fn tool(name: &str) -> Tool {
    Tool {
        name: name.to_string(),
        description: format!("does {name}"),
        input_schema: serde_json::json!({"type": "object", "properties": {}}),
    }
}

#[test]
fn manifest_reflects_brain_files_messages_and_tools() {
    let system = "You are an agent.\n## AGENTS.md\nrules\n## USER.md\nprofile";
    let request = LLMRequest::new(
        "replay-model",
        vec![
            Message::user("first question with several words".to_string()),
            Message::assistant("an answer".to_string()),
        ],
    )
    .with_system(system)
    // read_file/bash are CORE; browser_navigate is extended (not in CORE_TOOLS).
    .with_tools(vec![
        tool("read_file"),
        tool("bash"),
        tool("browser_navigate"),
    ]);

    let m = ContextManifest::from_request(&request);

    // Brain files detected by name in the system prompt.
    assert!(m.has_brain_file("AGENTS.md"));
    assert!(m.has_brain_file("USER.md"));
    assert!(!m.has_brain_file("CODE.md"));
    assert_eq!(m.brain_files_present.len(), 2);

    // Messages counted, with non-zero token estimate.
    assert_eq!(m.message_count, 2);
    assert!(m.message_tokens > 0);

    // Tools split into CORE vs extended.
    assert_eq!(m.tools.len(), 3);
    assert_eq!(m.core_tool_count, 2);
    assert_eq!(m.extended_tool_count, 1);
    assert!(m.has_tool("browser_navigate"));
    assert!(m.tools.iter().all(|t| t.schema_tokens > 0));

    // Totals are the sum of the parts.
    assert!(m.system_brain_tokens > 0);
    assert!(m.tool_schema_tokens > 0);
    assert_eq!(
        m.total_input_tokens,
        m.system_brain_tokens + m.message_tokens + m.tool_schema_tokens
    );
}

#[test]
fn empty_request_yields_zeroed_manifest() {
    let request = LLMRequest::new("replay-model", vec![]);
    let m = ContextManifest::from_request(&request);
    assert_eq!(m.system_brain_tokens, 0);
    assert_eq!(m.message_count, 0);
    assert!(m.tools.is_empty());
    assert_eq!(m.total_input_tokens, 0);
    assert!(m.brain_files_present.is_empty());
}

#[test]
fn manifest_serializes_to_json() {
    let request = LLMRequest::new("replay-model", vec![]).with_tools(vec![tool("read_file")]);
    let m = ContextManifest::from_request(&request);
    let json = serde_json::to_string(&m).unwrap();
    assert!(json.contains("\"core_tool_count\":1"));
}
