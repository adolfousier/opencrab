//! Integration tests for code-graph symbol extraction pipeline.
//!
//! Tests the full flow: index Rust file → extract symbols → store in DB →
//! query symbol graph → verify results.

#![cfg(feature = "code-graph")]

use crate::memory::db::Store;
use crate::memory::search::detect_structural_query;
use crate::memory::symbol_extractor::{SymbolExtractor, SymbolKind};
use tempfile::TempDir;

/// Sample Rust code with known symbols and call relationships.
const SAMPLE_CODE: &str = r#"
use std::collections::HashMap;

pub struct Config {
    pub name: String,
    pub value: i32,
}

pub trait Processor {
    fn process(&self, data: &str) -> Result<String, String>;
}

impl Processor for Config {
    fn process(&self, data: &str) -> Result<String, String> {
        Ok(format!("{}: {}", self.name, data))
    }
}

pub fn validate_input(input: &str) -> bool {
    !input.is_empty()
}

pub fn process_message(msg: &str) -> String {
    if validate_input(msg) {
        let config = Config {
            name: "test".to_string(),
            value: 42,
        };
        config.process(msg).unwrap_or_default()
    } else {
        String::new()
    }
}

pub fn main() {
    let result = process_message("hello");
    println!("{}", result);
}
"#;

#[test]
fn test_full_pipeline_extract_and_query() {
    // Create a temporary directory for the test database
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test_memory.db");

    // Open a fresh store
    let store = Store::open(&db_path).unwrap();
    store.ensure_symbol_tables().unwrap();

    // Create a sample Rust file
    let sample_file = temp_dir.path().join("sample.rs");
    std::fs::write(&sample_file, SAMPLE_CODE).unwrap();

    // Extract symbols using SymbolExtractor
    let mut extractor = SymbolExtractor::new().unwrap();
    let (symbols, call_edges) = extractor.extract(&sample_file, SAMPLE_CODE).unwrap();

    // Verify we extracted expected symbols
    let symbol_names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        symbol_names.contains(&"Config"),
        "Should extract Config struct"
    );
    assert!(
        symbol_names.contains(&"Processor"),
        "Should extract Processor trait"
    );
    assert!(
        symbol_names.contains(&"validate_input"),
        "Should extract validate_input function"
    );
    assert!(
        symbol_names.contains(&"process_message"),
        "Should extract process_message function"
    );

    // Verify we extracted call edges
    assert!(
        !call_edges.is_empty(),
        "Should extract at least one call edge"
    );
    let process_message_calls: Vec<_> = call_edges
        .iter()
        .filter(|e| e.caller == "process_message")
        .collect();
    assert!(
        !process_message_calls.is_empty(),
        "process_message should call other functions"
    );

    // Store symbols in database
    for symbol in &symbols {
        if symbol.kind != SymbolKind::Import {
            store
                .insert_symbol(
                    &symbol.name,
                    &symbol.kind.to_string(),
                    sample_file.to_str().unwrap(),
                    symbol.start_line,
                    symbol.end_line,
                )
                .unwrap();
        }
    }

    // Store call edges
    for edge in &call_edges {
        store
            .insert_call_edge(
                &edge.caller,
                &edge.callee,
                sample_file.to_str().unwrap(),
                edge.line,
            )
            .unwrap();
    }

    // Test structural query: who calls validate_input?
    let callers = store.query_callers_of("validate_input").unwrap();
    assert!(!callers.is_empty(), "Should find callers of validate_input");
    let caller_names: Vec<&str> = callers.iter().map(|(c, _, _)| c.as_str()).collect();
    assert!(
        caller_names.contains(&"process_message"),
        "process_message should call validate_input"
    );

    // Test structural query: what does process_message call?
    let callees = store.query_callees_of("process_message").unwrap();
    assert!(
        !callees.is_empty(),
        "Should find callees of process_message"
    );
    let callee_names: Vec<&str> = callees.iter().map(|(c, _, _)| c.as_str()).collect();
    assert!(
        callee_names.contains(&"validate_input"),
        "process_message should call validate_input"
    );

    // Test structural query: where is Config defined?
    let config_defs = store.query_symbols_by_name("Config").unwrap();
    assert!(!config_defs.is_empty(), "Should find Config definition");
    let (kind, file, start, _end) = &config_defs[0];
    assert_eq!(kind, "struct", "Config should be a struct");
    assert!(file.contains("sample.rs"), "Config should be in sample.rs");
    assert!(*start > 0, "Config should have a valid start line");
}

#[test]
fn test_structural_query_patterns() {
    // Test "who calls X"
    let result = detect_structural_query("who calls process_message");
    assert!(result.is_some());
    let (query_type, symbol) = result.unwrap();
    assert_eq!(query_type, "calls");
    assert_eq!(symbol, "process_message");

    // Test "what does X call"
    let result = detect_structural_query("what does validate_input call");
    assert!(result.is_some());
    let (query_type, symbol) = result.unwrap();
    assert_eq!(query_type, "called_by");
    assert_eq!(symbol, "validate_input");

    // Test "show implementations of X"
    let result = detect_structural_query("show implementations of Processor");
    assert!(result.is_some());
    let (query_type, symbol) = result.unwrap();
    assert_eq!(query_type, "implements");
    assert_eq!(symbol, "processor");

    // Test "where is X defined"
    let result = detect_structural_query("where is Config defined");
    assert!(result.is_some());
    let (query_type, symbol) = result.unwrap();
    assert_eq!(query_type, "defined_in");
    assert_eq!(symbol, "config");

    // Test conceptual query (should NOT match)
    let result = detect_structural_query("context compaction");
    assert!(
        result.is_none(),
        "Conceptual queries should not match structural patterns"
    );
}

#[test]
fn test_call_graph_integrity() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test_memory.db");
    let store = Store::open(&db_path).unwrap();
    store.ensure_symbol_tables().unwrap();

    // Insert symbols
    store
        .insert_symbol("main", "function", "main.rs", 1, 5)
        .unwrap();
    store
        .insert_symbol("process_message", "function", "main.rs", 10, 20)
        .unwrap();
    store
        .insert_symbol("validate_input", "function", "main.rs", 25, 30)
        .unwrap();

    // Insert call edges
    store
        .insert_call_edge("main", "process_message", "main.rs", 3)
        .unwrap();
    store
        .insert_call_edge("process_message", "validate_input", "main.rs", 12)
        .unwrap();

    // Query: who calls process_message?
    let callers = store.query_callers_of("process_message").unwrap();
    assert_eq!(callers.len(), 1);
    assert_eq!(callers[0].0, "main");

    // Query: what does main call?
    let callees = store.query_callees_of("main").unwrap();
    assert_eq!(callees.len(), 1);
    assert_eq!(callees[0].0, "process_message");

    // Query: who calls validate_input?
    let callers = store.query_callers_of("validate_input").unwrap();
    assert_eq!(callers.len(), 1);
    assert_eq!(callers[0].0, "process_message");
}

#[test]
fn test_symbol_kinds() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test_memory.db");
    let store = Store::open(&db_path).unwrap();
    store.ensure_symbol_tables().unwrap();

    // Insert different symbol kinds
    store
        .insert_symbol("MyStruct", "struct", "lib.rs", 1, 5)
        .unwrap();
    store
        .insert_symbol("MyEnum", "enum", "lib.rs", 10, 15)
        .unwrap();
    store
        .insert_symbol("MyTrait", "trait", "lib.rs", 20, 25)
        .unwrap();
    store
        .insert_symbol("process", "function", "lib.rs", 30, 40)
        .unwrap();
    store
        .insert_symbol("std::collections::HashMap", "import", "lib.rs", 1, 1)
        .unwrap();

    // Query by name
    let results = store.query_symbols_by_name("MyStruct").unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, "struct");

    let results = store.query_symbols_by_name("MyTrait").unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, "trait");

    let results = store.query_symbols_by_name("process").unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, "function");
}

/// Benchmark scaffolding: populate the LIVE profile store's symbol graph
/// from a real repository, bypassing the document layer (the external sweep
/// already indexed the files as FTS documents; this backfills symbols for
/// them in one pass).
///
/// Ignored by default — run explicitly with:
///   cargo test --features code-graph --lib populate_live_symbol_graph -- --ignored --nocapture
#[test]
#[ignore = "writes to the live profile memory.db — benchmark scaffolding"]
fn populate_live_symbol_graph() {
    let repo = "/Users/adolfousierstudio/srv/rs/opencrabs";
    let db_path = crate::config::opencrabs_home().join("memory/memory.db");
    let store = Store::open(&db_path).expect("open live store");
    store.ensure_symbol_tables().expect("ensure symbol tables");

    let mut files = 0usize;
    let mut failures = 0usize;
    let mut stack = vec![std::path::PathBuf::from(repo)];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                if name != "target" && name != ".git" && name != "node_modules" {
                    stack.push(path);
                }
            } else if name.ends_with(".rs") {
                let key = path.to_string_lossy().to_string();
                match std::fs::read_to_string(&path) {
                    Ok(body) => {
                        files += 1;
                        crate::memory::symbol_extractor::extract_and_store(&store, &key, &body);
                    }
                    Err(_) => failures += 1,
                }
            }
        }
    }

    let (symbols, edges, imports) = store.symbol_graph_counts().unwrap();
    println!(
        "populated: files={files} failures={failures} symbols={symbols} call_edges={edges} imports={imports}"
    );
    assert!(files > 1000, "expected the whole repo, got {files} files");
    assert!(symbols > 0, "no symbols extracted");
}
