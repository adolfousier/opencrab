//! Tree-sitter based symbol extraction for Rust source files.
//!
//! Parses `.rs` files into ASTs and extracts:
//! - Function definitions (name, file, line range)
//! - Function calls (caller → callee edges)
//! - Struct/enum/trait definitions
//! - impl blocks (trait implementations)
//! - Use statements (imports)
//!
//! Feature-gated behind `code-graph`.

#[cfg(feature = "code-graph")]
use std::path::Path;
#[cfg(feature = "code-graph")]
use tree_sitter::{Node, Parser};

/// A symbol extracted from source code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub file_path: String,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    Trait,
    Impl,
    Import,
}

impl std::fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SymbolKind::Function => write!(f, "function"),
            SymbolKind::Struct => write!(f, "struct"),
            SymbolKind::Enum => write!(f, "enum"),
            SymbolKind::Trait => write!(f, "trait"),
            SymbolKind::Impl => write!(f, "impl"),
            SymbolKind::Import => write!(f, "import"),
        }
    }
}

/// A call edge: caller function calls callee function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallEdge {
    pub caller: String,
    pub callee: String,
    pub file_path: String,
    pub line: usize,
}

/// Extracts symbols and call graphs from Rust source files using tree-sitter.
#[cfg(feature = "code-graph")]
pub struct SymbolExtractor {
    parser: Parser,
}

#[cfg(feature = "code-graph")]
impl SymbolExtractor {
    pub fn new() -> Result<Self, anyhow::Error> {
        let mut parser = Parser::new();
        let language = tree_sitter_rust::LANGUAGE;
        parser.set_language(&language.into())?;
        Ok(Self { parser })
    }

    /// Parse a Rust file and extract all symbols and call edges.
    pub fn extract(
        &mut self,
        file_path: &Path,
        source: &str,
    ) -> Result<(Vec<Symbol>, Vec<CallEdge>), anyhow::Error> {
        let tree = self
            .parser
            .parse(source, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse {}", file_path.display()))?;

        let file_path_str = file_path.to_string_lossy().to_string();
        let mut symbols = Vec::new();
        let mut call_edges = Vec::new();

        self.extract_from_node(
            tree.root_node(),
            source,
            &file_path_str,
            None, // no current function context at root
            &mut symbols,
            &mut call_edges,
        );

        Ok((symbols, call_edges))
    }

    fn extract_from_node(
        &self,
        node: Node,
        source: &str,
        file_path: &str,
        current_function: Option<&str>,
        symbols: &mut Vec<Symbol>,
        call_edges: &mut Vec<CallEdge>,
    ) {
        let kind = node.kind();

        match kind {
            "function_item" => {
                if let Some(name) = self.extract_function_name(node, source) {
                    let start_line = node.start_position().row;
                    let end_line = node.end_position().row;
                    symbols.push(Symbol {
                        name: name.clone(),
                        kind: SymbolKind::Function,
                        file_path: file_path.to_string(),
                        start_line,
                        end_line,
                    });

                    // Recurse into function body with this function as context
                    for child in node.children(&mut node.walk()) {
                        self.extract_from_node(
                            child,
                            source,
                            file_path,
                            Some(&name),
                            symbols,
                            call_edges,
                        );
                    }
                }
            }
            "struct_item" => {
                if let Some(name) = self.extract_type_name(node, source) {
                    symbols.push(Symbol {
                        name,
                        kind: SymbolKind::Struct,
                        file_path: file_path.to_string(),
                        start_line: node.start_position().row,
                        end_line: node.end_position().row,
                    });
                }
            }
            "enum_item" => {
                if let Some(name) = self.extract_type_name(node, source) {
                    symbols.push(Symbol {
                        name,
                        kind: SymbolKind::Enum,
                        file_path: file_path.to_string(),
                        start_line: node.start_position().row,
                        end_line: node.end_position().row,
                    });
                }
            }
            "trait_item" => {
                if let Some(name) = self.extract_type_name(node, source) {
                    symbols.push(Symbol {
                        name,
                        kind: SymbolKind::Trait,
                        file_path: file_path.to_string(),
                        start_line: node.start_position().row,
                        end_line: node.end_position().row,
                    });
                }
            }
            "impl_item" => {
                // Extract impl block (trait implementation)
                let impl_name = self.extract_impl_name(node, source);
                symbols.push(Symbol {
                    name: impl_name,
                    kind: SymbolKind::Impl,
                    file_path: file_path.to_string(),
                    start_line: node.start_position().row,
                    end_line: node.end_position().row,
                });
            }
            "use_declaration" => {
                // Extract import
                let import_path = self.extract_use_path(node, source);
                symbols.push(Symbol {
                    name: import_path,
                    kind: SymbolKind::Import,
                    file_path: file_path.to_string(),
                    start_line: node.start_position().row,
                    end_line: node.end_position().row,
                });
            }
            "call_expression" => {
                // Extract function call
                if let (Some(caller), Some(callee)) =
                    (current_function, self.extract_callee(node, source))
                {
                    call_edges.push(CallEdge {
                        caller: caller.to_string(),
                        callee,
                        file_path: file_path.to_string(),
                        line: node.start_position().row,
                    });
                }
            }
            _ => {
                // Recurse into children
                for child in node.children(&mut node.walk()) {
                    self.extract_from_node(
                        child,
                        source,
                        file_path,
                        current_function,
                        symbols,
                        call_edges,
                    );
                }
            }
        }
    }

    fn extract_function_name(&self, node: Node, source: &str) -> Option<String> {
        node.child_by_field_name("name")
            .map(|n| n.utf8_text(source.as_bytes()).unwrap_or("").to_string())
    }

    fn extract_type_name(&self, node: Node, source: &str) -> Option<String> {
        node.child_by_field_name("name")
            .map(|n| n.utf8_text(source.as_bytes()).unwrap_or("").to_string())
    }

    fn extract_impl_name(&self, node: Node, source: &str) -> String {
        // Try to extract "impl Trait for Type" or "impl Type"
        let mut result = String::from("impl");
        if let Some(type_node) = node.child_by_field_name("type") {
            let type_name = type_node.utf8_text(source.as_bytes()).unwrap_or("");
            result.push(' ');
            result.push_str(type_name);
        }
        result
    }

    fn extract_use_path(&self, node: Node, source: &str) -> String {
        // Extract the full use path (e.g., "std::collections::HashMap")
        node.child_by_field_name("argument")
            .map(|n| n.utf8_text(source.as_bytes()).unwrap_or("").to_string())
            .unwrap_or_else(|| "use".to_string())
    }

    fn extract_callee(&self, node: Node, source: &str) -> Option<String> {
        // Extract the function being called
        node.child_by_field_name("function")
            .map(|n| n.utf8_text(source.as_bytes()).unwrap_or("").to_string())
    }
}

#[cfg(all(test, feature = "code-graph"))]
mod tests {
    use super::*;
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
}
