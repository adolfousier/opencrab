use crate::brain::tools::Tool;
use crate::brain::tools::ToolCapability;
use crate::brain::tools::exa_search::*;

fn make_tool() -> ExaSearchTool {
    ExaSearchTool::new(None)
}

fn make_tool_with_key() -> ExaSearchTool {
    ExaSearchTool::new(Some("test-key".to_string()))
}

#[test]
fn test_tool_name() {
    let tool = make_tool();
    assert_eq!(tool.name(), "exa_search");
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
    let input = serde_json::json!({ "query": "  " });
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
fn test_validate_with_search_type() {
    let tool = make_tool();
    let input = serde_json::json!({
        "query": "test",
        "max_results": 3,
        "search_type": "neural"
    });
    assert!(tool.validate_input(&input).is_ok());
}

#[test]
fn test_default_deserialization() {
    let input: ExaSearchInput =
        serde_json::from_value(serde_json::json!({ "query": "hello" })).unwrap();
    assert_eq!(input.query, "hello");
    assert_eq!(input.max_results, 5);
    assert_eq!(input.search_type, "auto");
}

#[test]
fn test_mcp_mode_default() {
    let tool = make_tool();
    assert!(tool.use_mcp());
    assert!(tool.api_key.is_none());
}

#[test]
fn test_direct_api_mode_with_key() {
    let tool = make_tool_with_key();
    assert!(!tool.use_mcp());
    assert!(tool.api_key.is_some());
}

#[test]
fn test_parse_sse_response() {
    let sse_body = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"Search results here\"}],\"isError\":false}}\n\n";
    let json = ExaSearchTool::parse_sse_response(sse_body).unwrap();
    assert_eq!(json["id"], 2);
    assert_eq!(json["result"]["content"][0]["text"], "Search results here");
}

#[test]
fn test_parse_sse_response_no_data() {
    let sse_body = "event: ping\n\n";
    assert!(ExaSearchTool::parse_sse_response(sse_body).is_err());
}

#[test]
fn test_extract_mcp_result_success() {
    let json = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "result": {
            "content": [{ "type": "text", "text": "1. Result Title\n   URL: https://example.com\n" }],
            "isError": false
        }
    });
    let result = ExaSearchTool::extract_mcp_result(&json, "test query").unwrap();
    assert!(result.success);
}

#[test]
fn test_extract_mcp_result_error() {
    let json = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "error": { "code": -32602, "message": "Unknown tool" }
    });
    let result = ExaSearchTool::extract_mcp_result(&json, "test").unwrap();
    assert!(!result.success);
}

#[test]
fn test_extract_mcp_result_tool_error() {
    let json = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "result": {
            "content": [{ "type": "text", "text": "Rate limit exceeded" }],
            "isError": true
        }
    });
    let result = ExaSearchTool::extract_mcp_result(&json, "test").unwrap();
    assert!(!result.success);
}
