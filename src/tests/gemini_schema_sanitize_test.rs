//! Issue #99: Gemini's `function_declarations[].parameters` validator
//! rejects `additionalProperties`. The sanitizer in `gemini.rs` must
//! strip that key from every nested object before the request goes on
//! the wire, while leaving the rest of the schema intact.

use crate::brain::provider::gemini::sanitize_schema_for_gemini;
use serde_json::json;

#[test]
fn strips_top_level_additional_properties() {
    let schema = json!({
        "type": "object",
        "properties": { "x": { "type": "string" } },
        "additionalProperties": { "type": "string" }
    });
    let out = sanitize_schema_for_gemini(schema);
    assert!(out.get("additionalProperties").is_none());
    assert_eq!(out["type"], "object");
    assert_eq!(out["properties"]["x"]["type"], "string");
}

#[test]
fn strips_nested_additional_properties() {
    let schema = json!({
        "type": "object",
        "properties": {
            "headers": {
                "type": "object",
                "additionalProperties": { "type": "string" }
            },
            "query": {
                "type": "object",
                "additionalProperties": { "type": "string" }
            }
        }
    });
    let out = sanitize_schema_for_gemini(schema);
    assert!(
        out["properties"]["headers"]
            .get("additionalProperties")
            .is_none()
    );
    assert!(
        out["properties"]["query"]
            .get("additionalProperties")
            .is_none()
    );
    // Surrounding shape preserved.
    assert_eq!(out["properties"]["headers"]["type"], "object");
}

#[test]
fn strips_additional_properties_inside_arrays_of_schemas() {
    let schema = json!({
        "type": "array",
        "items": {
            "type": "object",
            "additionalProperties": { "type": "number" },
            "properties": { "k": { "type": "string" } }
        }
    });
    let out = sanitize_schema_for_gemini(schema);
    assert!(out["items"].get("additionalProperties").is_none());
    assert_eq!(out["items"]["properties"]["k"]["type"], "string");
}

#[test]
fn leaves_unrelated_keys_intact() {
    let schema = json!({
        "type": "object",
        "properties": {
            "name": { "type": "string", "description": "the name" },
            "count": { "type": "integer", "minimum": 0 }
        },
        "required": ["name"]
    });
    let out = sanitize_schema_for_gemini(schema.clone());
    assert_eq!(out, schema, "no additionalProperties present → no mutation");
}

#[test]
fn strips_in_deeply_nested_oneof_style_shapes() {
    // Even though Gemini doesn't support oneOf, we don't strip it
    // (that's a separate concern). But additionalProperties INSIDE
    // such a branch must still go.
    let schema = json!({
        "type": "object",
        "properties": {
            "a": {
                "type": "object",
                "properties": {
                    "b": {
                        "type": "object",
                        "additionalProperties": true
                    }
                }
            }
        }
    });
    let out = sanitize_schema_for_gemini(schema);
    assert!(
        out["properties"]["a"]["properties"]["b"]
            .get("additionalProperties")
            .is_none()
    );
}

#[test]
fn handles_value_types_other_than_object() {
    // Boolean / string / number / null at top level should pass
    // through unchanged.
    assert_eq!(sanitize_schema_for_gemini(json!(true)), json!(true));
    assert_eq!(sanitize_schema_for_gemini(json!("string")), json!("string"));
    assert_eq!(sanitize_schema_for_gemini(json!(42)), json!(42));
    assert_eq!(sanitize_schema_for_gemini(json!(null)), json!(null));
}

#[test]
fn http_tool_schema_smoke_test() {
    // Mirrors the real http tool schema shape from src/brain/tools/http.rs
    let schema = json!({
        "type": "object",
        "properties": {
            "url": { "type": "string" },
            "method": { "type": "string" },
            "headers": {
                "type": "object",
                "additionalProperties": { "type": "string" }
            },
            "query": {
                "type": "object",
                "additionalProperties": { "type": "string" }
            }
        },
        "required": ["url"]
    });
    let out = sanitize_schema_for_gemini(schema);
    let s = serde_json::to_string(&out).unwrap();
    assert!(
        !s.contains("additionalProperties"),
        "no additionalProperties may survive anywhere in the schema"
    );
    assert!(s.contains("\"url\""), "url field preserved");
    assert!(s.contains("\"headers\""), "headers field preserved");
}
