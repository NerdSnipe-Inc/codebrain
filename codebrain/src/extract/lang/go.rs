//! Tree-sitter AST walker for Go (.go) files.
//!
//! Extracted node types:
//!   - `function_declaration`  → NodeType::Function
//!   - `method_declaration`    → NodeType::Method (+ Contains edge from receiver type)
//!   - `type_declaration` with struct body → NodeType::Struct
//!   - `import_declaration`    → EdgeKind::Imports
//!
//! Interface implementations in Go are implicit (structural typing), so
//! Implements edges are not extracted — they would all be Ambiguous.

use tree_sitter::{Node, Parser};

use crate::extract::ExtractionResult;
use crate::types::{EdgeConfidence, EdgeKind, GraphEdge, NodeData, NodeType};

pub fn extract(source: &str, file: &str) -> ExtractionResult {
    let language = tree_sitter_go::language();
    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        tracing::warn!(file, "go extractor: failed to load grammar");
        return ExtractionResult::default();
    }

    let tree = match parser.parse(source.as_bytes(), None) {
        Some(t) => t,
        None => return ExtractionResult::default(),
    };

    let mut result = ExtractionResult::default();
    walk(tree.root_node(), source.as_bytes(), file, &mut result);
    result
}

fn walk(node: Node, source: &[u8], file: &str, result: &mut ExtractionResult) {
    match node.kind() {
        "import_declaration" => extract_imports(node, source, file, result),
        "function_declaration" => extract_function(node, source, file, result),
        "method_declaration"   => extract_method(node, source, file, result),
        "type_declaration"     => extract_type_decl(node, source, file, result),
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                walk(child, source, file, result);
            }
        }
    }
}

fn extract_imports(node: Node, source: &[u8], file: &str, result: &mut ExtractionResult) {
    // Handles both `import "path"` and `import ( "path1" "path2" )`
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "import_spec" | "interpreted_string_literal" => {
                let raw = text(child, source);
                let path = raw.trim_matches('"').trim_matches('`');
                if !path.is_empty() {
                    result.edges.push(GraphEdge {
                        from_id:    format!("{file}::__module__"),
                        to_id:      format!("{path}::__module__"),
                        kind:       EdgeKind::Imports,
                        confidence: EdgeConfidence::Extracted,
                    });
                }
            }
            "import_spec_list" => {
                // Walk the spec list recursively
                let mut inner = child.walk();
                for spec in child.children(&mut inner) {
                    if spec.kind() == "import_spec" {
                        // Path is the last child (the string literal)
                        let raw = text(spec, source);
                        let path = raw.split('"').nth(1).unwrap_or("").trim();
                        if !path.is_empty() {
                            result.edges.push(GraphEdge {
                                from_id:    format!("{file}::__module__"),
                                to_id:      format!("{path}::__module__"),
                                kind:       EdgeKind::Imports,
                                confidence: EdgeConfidence::Extracted,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn extract_function(node: Node, source: &[u8], file: &str, result: &mut ExtractionResult) {
    let name = match node.child_by_field_name("name") {
        Some(n) => text(n, source),
        None    => return,
    };

    result.nodes.push(NodeData {
        id:        node_id(file, &name),
        label:     name,
        node_type: NodeType::Function,
        file:      file.to_string(),
        line:      node.start_position().row + 1,
        community: 0,
    });
}

fn extract_method(node: Node, source: &[u8], file: &str, result: &mut ExtractionResult) {
    let name = match node.child_by_field_name("name") {
        Some(n) => text(n, source),
        None    => return,
    };

    let id = node_id(file, &name);

    result.nodes.push(NodeData {
        id: id.clone(),
        label: name,
        node_type: NodeType::Method,
        file: file.to_string(),
        line: node.start_position().row + 1,
        community: 0,
    });

    // Receiver: `func (r ReceiverType) MethodName(...)` → Contains edge
    if let Some(recv) = node.child_by_field_name("receiver") {
        // The receiver type name is typically the last named child of the
        // parameter_list inside the receiver spec.
        let recv_text = text(recv, source);
        // Rough extraction: take the last whitespace-separated token before `)`
        let receiver_type = recv_text
            .trim_matches(|c| c == '(' || c == ')')
            .split_whitespace()
            .last()
            .map(|t| t.trim_start_matches('*'))  // strip pointer receiver `*Foo`
            .unwrap_or("")
            .to_string();

        if !receiver_type.is_empty() {
            result.edges.push(GraphEdge {
                from_id:    node_id(file, &receiver_type),
                to_id:      id,
                kind:       EdgeKind::Contains,
                confidence: EdgeConfidence::Extracted,
            });
        }
    }
}

fn extract_type_decl(node: Node, source: &[u8], file: &str, result: &mut ExtractionResult) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "type_spec" {
            let name = match child.child_by_field_name("name") {
                Some(n) => text(n, source),
                None    => continue,
            };
            let type_node = child.child_by_field_name("type");
            let node_type = match type_node.map(|n| n.kind()) {
                Some("struct_type")    => NodeType::Struct,
                Some("interface_type") => NodeType::Class, // closest analogue
                _                      => NodeType::Variable,
            };

            result.nodes.push(NodeData {
                id:        node_id(file, &name),
                label:     name,
                node_type,
                file:      file.to_string(),
                line:      child.start_position().row + 1,
                community: 0,
            });
        }
    }
}

fn text(node: Node, source: &[u8]) -> String {
    node.utf8_text(source).unwrap_or("").to_string()
}

fn node_id(file: &str, symbol: &str) -> String {
    format!("{file}::{symbol}")
}
