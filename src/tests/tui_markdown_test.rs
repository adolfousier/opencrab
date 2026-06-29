use super::*;

#[test]
fn test_parse_simple_text() {
    let md = "Hello world";
    let lines = parse_markdown(md, 80);
    assert!(!lines.is_empty());
}

#[test]
fn test_parse_heading() {
    let md = "# Heading 1\n\nSome text";
    let lines = parse_markdown(md, 80);
    assert!(lines.len() > 1);
}

#[test]
fn test_parse_code_block() {
    let md = "```rust\nfn main() {}\n```";
    let lines = parse_markdown(md, 80);
    assert!(lines.len() > 2); // Header, code, footer
}

#[test]
fn test_parse_inline_code() {
    let md = "Use `cargo build` to compile";
    let lines = parse_markdown(md, 80);
    assert!(!lines.is_empty());
}

#[test]
fn test_parse_list() {
    let md = "- Item 1\n- Item 2\n- Item 3";
    let lines = parse_markdown(md, 80);
    assert!(lines.len() >= 3);
}

#[test]
fn test_parse_horizontal_rule() {
    let md = "Before\n\n---\n\nAfter";
    let lines = parse_markdown(md, 80);
    assert!(lines.len() > 2);
}

#[test]
fn test_empty_markdown() {
    let md = "";
    let lines = parse_markdown(md, 80);
    assert!(lines.is_empty() || lines.iter().all(|l| l.spans.is_empty()));
}

#[test]
fn test_table_wide_columnar() {
    let md = "| name | age |\n|---|---|\n| Alice | 30 |\n| Bob | 25 |";
    let lines = parse_markdown(md, 80);
    // Should contain box-drawing chars
    let text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
        .collect();
    assert!(text.contains('┌'), "Should have top border");
    assert!(text.contains('│'), "Should have cell borders");
    assert!(text.contains('└'), "Should have bottom border");
}

#[test]
fn test_table_narrow_card() {
    let md = "| name | department | location | salary |\n|---|---|---|---|\n| Alice | Engineering | San Francisco | $145,000 |";
    let lines = parse_markdown(md, 30); // Too narrow for table
    // Should render as card format: "Header: Value"
    let text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
        .collect();
    assert!(text.contains("name"), "Should have header as label");
    assert!(text.contains(": "), "Should have key:value separator");
    assert!(!text.contains('┌'), "Should NOT have box borders");
}
