use crate::brain::tools::Tool;
use crate::brain::tools::ToolCapability;
use crate::brain::tools::brave_search::*;

fn make_tool() -> BraveSearchTool {
    BraveSearchTool::new("test-key".to_string())
}

#[test]
fn test_tool_name() {
    let tool = make_tool();
    assert_eq!(tool.name(), "brave_search");
}

#[test]
fn test_tool_capabilities() {
    let tool = make_tool();
    let caps = tool.capabilities();
    assert_eq!(caps.len(), 1);
    assert!(matches!(caps[0], ToolCapability::Network));
}

#[test]
fn test_tool_no_approval_required() {
    let tool = make_tool();
    assert!(!tool.requires_approval());
}

#[test]
fn test_input_schema_has_query() {
    let tool = make_tool();
    let schema = tool.input_schema();
    let required = schema.get("required").and_then(|v| v.as_array());
    assert!(required.is_some());
    let required = required.unwrap();
    assert!(required.iter().any(|v| v.as_str() == Some("query")));
}

#[test]
fn test_validate_valid_input() {
    let tool = make_tool();
    let input = serde_json::json!({ "query": "rust programming" });
    assert!(tool.validate_input(&input).is_ok());
}

#[test]
fn test_validate_empty_query() {
    let tool = make_tool();
    let input = serde_json::json!({ "query": "" });
    assert!(tool.validate_input(&input).is_err());
}

#[test]
fn test_validate_missing_query() {
    let tool = make_tool();
    let input = serde_json::json!({ "max_results": 5 });
    assert!(tool.validate_input(&input).is_err());
}

#[test]
fn test_validate_max_results_zero() {
    let tool = make_tool();
    let input = serde_json::json!({ "query": "test", "max_results": 0 });
    assert!(tool.validate_input(&input).is_err());
}

#[test]
fn test_validate_max_results_too_high() {
    let tool = make_tool();
    let input = serde_json::json!({ "query": "test", "max_results": 11 });
    assert!(tool.validate_input(&input).is_err());
}

#[test]
fn test_default_deserialization() {
    let input: BraveSearchInput =
        serde_json::from_value(serde_json::json!({ "query": "hello" })).unwrap();
    assert_eq!(input.query, "hello");
    assert_eq!(input.max_results, 5);
}

#[test]
fn test_brave_response_parsing() {
    let json = serde_json::json!({
        "web": {
            "results": [
                {
                    "title": "Test Result",
                    "url": "https://example.com",
                    "description": "A test result"
                }
            ]
        }
    });
    let response: BraveResponse = serde_json::from_value(json).unwrap();
    let results = response.web.unwrap().results;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Test Result");
    assert_eq!(results[0].url, "https://example.com");
    assert_eq!(results[0].description, Some("A test result".to_string()));
}

#[test]
fn test_brave_response_no_web() {
    let json = serde_json::json!({});
    let response: BraveResponse = serde_json::from_value(json).unwrap();
    assert!(response.web.is_none());
}
