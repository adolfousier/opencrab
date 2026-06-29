use super::*;

#[test]
fn passes_through_valid_json() {
    let v = parse_or_repair(r#"{"a":1,"b":"x"}"#);
    assert_eq!(v["a"], 1);
    assert_eq!(v["b"], "x");
}

#[test]
fn empty_returns_object() {
    let v = parse_or_repair("");
    assert!(v.is_object());
}

#[test]
fn closes_open_string() {
    let v = parse_or_repair(r#"{"command":"git status"#);
    assert_eq!(v["command"], "git status");
}

#[test]
fn closes_missing_brace() {
    let v = parse_or_repair(r#"{"a":1,"b":2"#);
    assert_eq!(v["a"], 1);
    assert_eq!(v["b"], 2);
}

#[test]
fn drops_trailing_key_without_value() {
    let v = parse_or_repair(r#"{"a":1,"b":"#);
    assert_eq!(v["a"], 1);
    assert!(v.get("b").is_none());
}

#[test]
fn closes_nested_array() {
    let v = parse_or_repair(r#"{"items":[1,2,3"#);
    assert_eq!(v["items"][0], 1);
    assert_eq!(v["items"][2], 3);
}

#[test]
fn closes_string_inside_array() {
    let v = parse_or_repair(r#"{"items":["a","b"#);
    assert_eq!(v["items"][0], "a");
    assert_eq!(v["items"][1], "b");
}

#[test]
fn unrecoverable_returns_partial_envelope() {
    let v = parse_or_repair(r#"this is not json"#);
    assert!(v["_repair_failed"].as_bool().unwrap_or(false));
    assert_eq!(v["_partial"], "this is not json");
}

#[test]
fn handles_escaped_quote_in_string() {
    let v = parse_or_repair(r#"{"msg":"he said \"hi"#);
    assert_eq!(v["msg"], "he said \"hi");
}

#[test]
fn strips_trailing_comma() {
    let v = parse_or_repair(r#"{"a":1,"#);
    assert_eq!(v["a"], 1);
}
