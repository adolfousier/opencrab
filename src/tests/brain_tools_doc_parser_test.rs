use crate::brain::tools::Tool;
use crate::brain::tools::ToolExecutionContext;
use crate::brain::tools::doc_parser::*;
use std::io::Write;
use tempfile::NamedTempFile;
use tokio;
use uuid::Uuid;

#[tokio::test]
async fn test_parse_text_file() {
    let mut temp_file = NamedTempFile::with_suffix(".txt").unwrap();
    writeln!(temp_file, "This is a test document.\nWith multiple lines.").unwrap();
    temp_file.flush().unwrap();

    let tool = DocParserTool;
    let session_id = Uuid::new_v4();
    let context = ToolExecutionContext::new(session_id);

    let input = serde_json::json!({
        "path": temp_file.path().to_str().unwrap()
    });

    let result = tool.execute(input, &context).await.unwrap();
    assert!(result.success);
    assert!(result.output.contains("This is a test document"));
    assert!(result.output.contains("multiple lines"));
}

#[tokio::test]
async fn test_parse_markdown_file() {
    let mut temp_file = NamedTempFile::with_suffix(".md").unwrap();
    writeln!(temp_file, "# Header\n\nSome **bold** text.").unwrap();
    temp_file.flush().unwrap();

    let tool = DocParserTool;
    let session_id = Uuid::new_v4();
    let context = ToolExecutionContext::new(session_id);

    let input = serde_json::json!({
        "path": temp_file.path().to_str().unwrap()
    });

    let result = tool.execute(input, &context).await.unwrap();
    assert!(result.success);
    assert!(result.output.contains("# Header"));
    assert!(result.output.contains("**bold**"));
}

#[tokio::test]
async fn test_parse_json_file() {
    let mut temp_file = NamedTempFile::with_suffix(".json").unwrap();
    writeln!(temp_file, r#"{{"name": "test", "value": 42}}"#).unwrap();
    temp_file.flush().unwrap();

    let tool = DocParserTool;
    let session_id = Uuid::new_v4();
    let context = ToolExecutionContext::new(session_id);

    let input = serde_json::json!({
        "path": temp_file.path().to_str().unwrap()
    });

    let result = tool.execute(input, &context).await.unwrap();
    assert!(result.success);
    assert!(result.output.contains("\"name\""));
    assert!(result.output.contains("\"test\""));
}

#[tokio::test]
async fn test_parse_html_file() {
    let mut temp_file = NamedTempFile::with_suffix(".html").unwrap();
    writeln!(
        temp_file,
        "<html><head><title>Test Page</title></head><body><p>Hello World</p></body></html>"
    )
    .unwrap();
    temp_file.flush().unwrap();

    let tool = DocParserTool;
    let session_id = Uuid::new_v4();
    let context = ToolExecutionContext::new(session_id);

    let input = serde_json::json!({
        "path": temp_file.path().to_str().unwrap(),
        "include_metadata": true
    });

    let result = tool.execute(input, &context).await.unwrap();
    assert!(result.success);
    assert!(result.output.contains("Test Page"));
    assert!(result.output.contains("Hello World"));
}

#[tokio::test]
async fn test_max_chars_truncation() {
    let mut temp_file = NamedTempFile::with_suffix(".txt").unwrap();
    writeln!(
        temp_file,
        "This is a very long document that should be truncated."
    )
    .unwrap();
    temp_file.flush().unwrap();

    let tool = DocParserTool;
    let session_id = Uuid::new_v4();
    let context = ToolExecutionContext::new(session_id);

    let input = serde_json::json!({
        "path": temp_file.path().to_str().unwrap(),
        "max_chars": 10
    });

    let result = tool.execute(input, &context).await.unwrap();
    assert!(result.success);
    assert!(result.output.contains("Truncated"));
}

#[tokio::test]
async fn test_unsupported_format() {
    let mut temp_file = NamedTempFile::with_suffix(".xyz").unwrap();
    writeln!(temp_file, "Some content").unwrap();
    temp_file.flush().unwrap();

    let tool = DocParserTool;
    let session_id = Uuid::new_v4();
    let context = ToolExecutionContext::new(session_id);

    let input = serde_json::json!({
        "path": temp_file.path().to_str().unwrap()
    });

    let result = tool.execute(input, &context).await.unwrap();
    assert!(!result.success);
    assert!(result.error.unwrap().contains("Unsupported"));
}

#[tokio::test]
async fn test_nonexistent_file() {
    let tool = DocParserTool;
    let session_id = Uuid::new_v4();
    let context = ToolExecutionContext::new(session_id);

    let input = serde_json::json!({
        "path": "/nonexistent/document.pdf"
    });

    let result = tool.execute(input, &context).await.unwrap();
    assert!(!result.success);
    assert!(result.error.unwrap().contains("not found"));
}

#[test]
fn test_tool_schema() {
    let tool = DocParserTool;
    assert_eq!(tool.name(), "parse_document");
    assert!(!tool.requires_approval());

    let schema = tool.input_schema();
    assert!(schema.is_object());
    assert!(schema["properties"]["path"].is_object());
}

#[test]
fn test_strip_html_tags() {
    let html = "<html><body><p>Hello</p><script>var x=1;</script><p>World</p></body></html>";
    let text = DocParserTool::strip_html_tags(html);
    assert!(text.contains("Hello"));
    assert!(text.contains("World"));
    assert!(!text.contains("var x"));
}

#[test]
fn test_extract_html_title() {
    let html = "<html><head><title>My Document</title></head><body></body></html>";
    let title = DocParserTool::extract_html_title(html);
    assert_eq!(title, Some("My Document".to_string()));
}
