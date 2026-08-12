//! `memory_search` must be able to search brain files, not only daily logs
//! (#1020).
//!
//! `search_brain` existed, was used internally by `brain/hints.rs`, and was not
//! reachable by the agent. So "does a rule about this already exist" went
//! through the daily-log corpus, where 158 notes outrank nine brain files and
//! reuse the same words for unrelated things. Measured on a live index, the
//! agent-reachable path found the target rule on 2 of 5 phrasings; the same
//! queries scoped to brain files found it 5 of 5, for the same payload.
//!
//! The failure was silent — three confident irrelevant hits rather than none —
//! which is what made it expensive: an agent checking before appending a rule
//! reads "nothing similar" and writes the duplicate (#1017).
//!
//! A third search tool would have been the wrong fix; every tool schema is paid
//! for in context on every request. These pin the scope contract on the tool
//! that already exists.

use crate::brain::tools::memory_search::MemorySearchTool;
use crate::brain::tools::r#trait::Tool;

/// The three corpora must be declared, so the model can pick one.
#[test]
fn the_schema_offers_all_three_scopes() {
    let schema = MemorySearchTool.input_schema();
    let scope = &schema["properties"]["scope"];
    let values: Vec<String> = scope["enum"]
        .as_array()
        .expect("scope must be an enum")
        .iter()
        .map(|v| v.as_str().unwrap_or_default().to_string())
        .collect();
    for want in ["memory", "brain", "all"] {
        assert!(values.contains(&want.to_string()), "missing scope: {want}");
    }
}

/// The default must stay `memory`, or every existing caller silently changes
/// corpus.
#[test]
fn the_default_scope_is_unchanged() {
    let schema = MemorySearchTool.input_schema();
    assert_eq!(schema["properties"]["scope"]["default"], "memory");
    assert!(
        !schema["required"]
            .as_array()
            .expect("required list")
            .iter()
            .any(|v| v == "scope"),
        "scope must stay optional"
    );
}

/// The description must say which scope answers which question.
///
/// Picking the wrong corpus is the failure this issue is about, and the model
/// only has the description to go on.
#[test]
fn the_description_routes_rules_to_the_brain_scope() {
    let d = MemorySearchTool.description().to_lowercase();
    assert!(d.contains("brain"), "must name the brain scope");
    assert!(
        d.contains("rule"),
        "must say the brain scope is where rules live"
    );
    assert!(
        d.contains("load_brain_file"),
        "must point at the tool that reads a full section once a hit locates it"
    );
}
