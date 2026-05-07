//! Tree-sitter AST walker for JavaScript (.js / .jsx) files.
//!
//! JavaScript shares most grammar node types with TypeScript, so this module
//! delegates to the TypeScript walker with `tsx = false` after parsing with
//! the JavaScript grammar. The key difference is the absence of type
//! annotations, which makes symbol extraction slightly simpler.

use tree_sitter::{Node, Parser};

use crate::extract::ExtractionResult;
use crate::types::{EdgeConfidence, EdgeKind, GraphEdge, NodeData, NodeType};

pub fn extract(source: &str, file: &str) -> ExtractionResult {
    let language = tree_sitter_javascript::language();
    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        tracing::warn!(file, "js extractor: failed to load grammar");
        return ExtractionResult::default();
    }

    let tree = match parser.parse(source.as_bytes(), None) {
        Some(t) => t,
        None => return ExtractionResult::default(),
    };

    let mut result = ExtractionResult::default();
    walk(tree.root_node(), source.as_bytes(), file, None, &mut result);
    result
}

fn walk(
    node:         Node,
    source:       &[u8],
    file:         &str,
    class_target: Option<&str>,
    result:       &mut ExtractionResult,
) {
    match node.kind() {
        "import_statement"     => extract_import(node, source, file, result),
        "function_declaration" => extract_function(node, source, file, class_target, result),
        "class_declaration"    => extract_class(node, source, file, result),
        "method_definition"    => extract_method(node, source, file, class_target, result),
        "export_statement"     => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                walk(child, source, file, class_target, result);
            }
        }
        "lexical_declaration"
        | "variable_declaration" => extract_variable_decl(node, source, file, result),
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                walk(child, source, file, class_target, result);
            }
        }
    }
}

fn extract_import(node: Node, source: &[u8], file: &str, result: &mut ExtractionResult) {
    let source_str = node
        .child_by_field_name("source")
        .map(|n| text(n, source).trim_matches('"').trim_matches('\'').to_string())
        .unwrap_or_default();

    if source_str.is_empty() { return; }

    result.edges.push(GraphEdge {
        from_id:    format!("{file}::__module__"),
        to_id:      format!("{source_str}::__module__"),
        kind:       EdgeKind::Imports,
        confidence: EdgeConfidence::Extracted,
    });
}

fn extract_function(
    node:         Node,
    source:       &[u8],
    file:         &str,
    class_target: Option<&str>,
    result:       &mut ExtractionResult,
) {
    let name = match node.child_by_field_name("name") {
        Some(n) => text(n, source),
        None    => return,
    };

    let id = node_id(file, &name);
    result.nodes.push(NodeData {
        id: id.clone(),
        label: name,
        node_type: if class_target.is_some() { NodeType::Method } else { NodeType::Function },
        file: file.to_string(),
        line: node.start_position().row + 1,
        community: 0,
    });

    if let Some(target) = class_target {
        result.edges.push(GraphEdge {
            from_id:    node_id(file, target),
            to_id:      id,
            kind:       EdgeKind::Contains,
            confidence: EdgeConfidence::Extracted,
        });
    }
}

fn extract_class(node: Node, source: &[u8], file: &str, result: &mut ExtractionResult) {
    let name = match node.child_by_field_name("name") {
        Some(n) => text(n, source),
        None    => return,
    };

    let id = node_id(file, &name);
    result.nodes.push(NodeData {
        id: id.clone(),
        label: name.clone(),
        node_type: NodeType::Class,
        file: file.to_string(),
        line: node.start_position().row + 1,
        community: 0,
    });

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, source, file, Some(&name.clone()), result);
    }
}

fn extract_method(
    node:         Node,
    source:       &[u8],
    file:         &str,
    class_target: Option<&str>,
    result:       &mut ExtractionResult,
) {
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

    if let Some(target) = class_target {
        result.edges.push(GraphEdge {
            from_id:    node_id(file, target),
            to_id:      id,
            kind:       EdgeKind::Contains,
            confidence: EdgeConfidence::Extracted,
        });
    }
}

fn extract_variable_decl(node: Node, source: &[u8], file: &str, result: &mut ExtractionResult) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "variable_declarator" {
            let name_node  = match child.child_by_field_name("name") { Some(n) => n, None => continue };
            let value_node = match child.child_by_field_name("value") { Some(v) => v, None => continue };
            if matches!(value_node.kind(), "arrow_function" | "function_expression") {
                let name = text(name_node, source);
                if !name.is_empty() {
                    result.nodes.push(NodeData {
                        id:        node_id(file, &name),
                        label:     name,
                        node_type: NodeType::Function,
                        file:      file.to_string(),
                        line:      child.start_position().row + 1,
                        community: 0,
                    });
                }
            }
        }
    }
}

fn text(node: Node, source: &[u8]) -> String {
    node.utf8_text(source).unwrap_or("").to_string()
}

fn node_id(file: &str, symbol: &str) -> String {
    format!("{file}::{symbol}")
}
