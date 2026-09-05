//! tree-sitter symbol and call-edge extraction (code-graph): fixtures are raw source strings, byte-identical to the inline originals.

use crate::memory::symbol_extractor::*;
use std::path::PathBuf;

#[test]
fn test_extract_function() {
    let source = r#"
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
"#;
    let mut extractor = SymbolExtractor::new().unwrap();
    let path = PathBuf::from("test.rs");
    let (symbols, _edges) = extractor.extract(&path, source).unwrap();

    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].name, "add");
    assert_eq!(symbols[0].kind, SymbolKind::Function);
}

#[test]
fn test_extract_struct() {
    let source = r#"
pub struct Point {
    x: f64,
    y: f64,
}
"#;
    let mut extractor = SymbolExtractor::new().unwrap();
    let path = PathBuf::from("test.rs");
    let (symbols, _edges) = extractor.extract(&path, source).unwrap();

    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].name, "Point");
    assert_eq!(symbols[0].kind, SymbolKind::Struct);
}

#[test]
fn test_extract_call_edge() {
    let source = r#"
fn caller() {
    callee();
}

fn callee() {
    println!("called");
}
"#;
    let mut extractor = SymbolExtractor::new().unwrap();
    let path = PathBuf::from("test.rs");
    let (symbols, edges) = extractor.extract(&path, source).unwrap();

    assert_eq!(symbols.len(), 2); // caller, callee
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].caller, "caller");
    assert_eq!(edges[0].callee, "callee");
}

#[test]
fn test_extract_trait() {
    let source = r#"
pub trait Drawable {
    fn draw(&self);
}
"#;
    let mut extractor = SymbolExtractor::new().unwrap();
    let path = PathBuf::from("test.rs");
    let (symbols, _edges) = extractor.extract(&path, source).unwrap();

    assert!(
        symbols
            .iter()
            .any(|s| s.name == "Drawable" && s.kind == SymbolKind::Trait)
    );
}

#[test]
fn test_extract_impl() {
    let source = r#"
struct Circle;

impl Drawable for Circle {
    fn draw(&self) {}
}
"#;
    let mut extractor = SymbolExtractor::new().unwrap();
    let path = PathBuf::from("test.rs");
    let (symbols, _edges) = extractor.extract(&path, source).unwrap();

    assert!(symbols.iter().any(|s| s.kind == SymbolKind::Impl));
}

#[test]
fn test_method_call_normalized_to_bare_name() {
    // `store.insert_symbol(..)` must land as callee `insert_symbol`
    // (receiver-qualified storage broke callers-of queries — benchmark
    // found 0 callers of insert_symbol despite 37k edges).
    let source = r#"
fn populate(store: &Store) {
    store.insert_symbol("a", "function", "f.rs", 1, 2);
}
"#;
    let mut extractor = SymbolExtractor::new().unwrap();
    let path = PathBuf::from("test.rs");
    let (_symbols, edges) = extractor.extract(&path, source).unwrap();

    assert!(
        edges
            .iter()
            .any(|e| e.caller == "populate" && e.callee == "insert_symbol")
    );
}

#[test]
fn test_enum_variant_constructors_skipped() {
    // Some/Ok/Err are not functions; they polluted the top of the
    // callee table (1278 + 698 edges) before the denylist.
    let source = r#"
fn wrap(x: u32) -> Option<u32> {
    let a = Some(x);
    let b = Ok(1u32);
    inner(a)
}
fn inner(_v: Option<u32>) {}
"#;
    let mut extractor = SymbolExtractor::new().unwrap();
    let path = PathBuf::from("test.rs");
    let (_symbols, edges) = extractor.extract(&path, source).unwrap();

    assert!(!edges.iter().any(|e| e.callee == "Some" || e.callee == "Ok"));
    assert!(
        edges
            .iter()
            .any(|e| e.caller == "wrap" && e.callee == "inner")
    );
}

#[test]
fn test_module_path_call_kept_full() {
    // `Arc::new` has no `.` receiver — a true module path, kept intact.
    let source = r#"
use std::sync::Arc;
fn share() -> Arc<u32> {
    Arc::new(1u32)
}
"#;
    let mut extractor = SymbolExtractor::new().unwrap();
    let path = PathBuf::from("test.rs");
    let (_symbols, edges) = extractor.extract(&path, source).unwrap();

    assert!(
        edges
            .iter()
            .any(|e| e.caller == "share" && e.callee == "Arc::new")
    );
}

#[test]
fn test_await_context_chain_inner_call_extracted() {
    // #1325: `retry_db_operation(..).await.context(..)` — the inner call
    // sits inside the outer `.context` call's subtree (receiver
    // await_expression). The call_expression arm must recurse after
    // handling, or the inner edge is dropped. Generics in the signature
    // are incidental; the nesting is the bug.
    let source = r#"
async fn retry_db_anyhow<F, Fut, T>(operation: F, config: &DbRetryConfig) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    retry_db_operation(operation, config)
        .await
        .context("Database operation failed after retries")
}
"#;
    let mut extractor = SymbolExtractor::new().unwrap();
    let path = PathBuf::from("test.rs");
    let (_symbols, edges) = extractor.extract(&path, source).unwrap();

    assert!(
        edges
            .iter()
            .any(|e| e.caller == "retry_db_anyhow" && e.callee == "retry_db_operation"),
        "inner call of an .await.context() chain must produce its edge, got: {edges:?}"
    );
    assert!(edges.iter().any(|e| e.callee == "context"));
}

#[test]
fn test_nested_call_in_arguments_extracted() {
    // #1325 (same class): `wrap(inner())` — inner call lives in the outer
    // call's argument subtree.
    let source = r#"
fn outer() -> u32 {
    wrap(inner())
}
"#;
    let mut extractor = SymbolExtractor::new().unwrap();
    let path = PathBuf::from("test.rs");
    let (_symbols, edges) = extractor.extract(&path, source).unwrap();

    assert!(
        edges
            .iter()
            .any(|e| e.caller == "outer" && e.callee == "wrap")
    );
    assert!(
        edges
            .iter()
            .any(|e| e.caller == "outer" && e.callee == "inner")
    );
}

#[test]
fn test_impl_method_symbol_and_calls_extracted() {
    // #1325 (found while fixing): impl_item handled-and-stopped, so impl
    // methods had no symbol rows and their bodies no call edges.
    let source = r#"
struct Store;

impl Store {
    fn query_all(&self) -> Vec<u32> {
        self.load()
    }
}
"#;
    let mut extractor = SymbolExtractor::new().unwrap();
    let path = PathBuf::from("test.rs");
    let (symbols, edges) = extractor.extract(&path, source).unwrap();

    assert!(
        symbols.iter().any(|s| s.name == "query_all"),
        "impl method must appear in symbols, got: {symbols:?}"
    );
    assert!(
        edges
            .iter()
            .any(|e| e.caller == "query_all" && e.callee == "load")
    );
}
