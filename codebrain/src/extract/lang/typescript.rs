//! Tree-sitter AST walker for TypeScript (.ts) and TSX (.tsx) files.
//!
//! Extracted node types:
//!   - `function_declaration` / `function_expression`  → NodeType::Function
//!   - `arrow_function` (named, via variable_declarator) → NodeType::Function
//!   - `class_declaration`                              → NodeType::Class
//!   - `method_definition`                              → NodeType::Method
//!   - `import_statement`                               → EdgeKind::Imports
//!   - `export_statement` wrapping any of the above — unwrapped transparently
//!
//! Call edges are not extracted for TypeScript because call-site resolution
//! requires the full TypeScript type checker (tsc); the tree-sitter grammar
//! alone cannot resolve method receivers. Use semantic extraction instead.

use tree_sitter::{Node, Parser};

use crate::extract::ExtractionResult;
use crate::types::{EdgeConfidence, EdgeKind, GraphEdge, NodeData, NodeType};

pub fn extract(source: &str, file: &str, tsx: bool) -> ExtractionResult {
    let language = if tsx {
        tree_sitter_typescript::language_tsx()
    } else {
        tree_sitter_typescript::language_typescript()
    };

    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        tracing::warn!(file, "ts extractor: failed to load grammar");
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
        "import_statement"    => extract_import(node, source, file, result),
        "function_declaration"
        | "function_expression" => extract_function(node, source, file, class_target, result),
        "arrow_function"      => extract_arrow(node, source, file, result),
        "class_declaration"
        | "class_expression"  => extract_class(node, source, file, result),
        "method_definition"   => extract_method(node, source, file, class_target, result),
        "export_statement"    => {
            // Transparent: walk the exported declaration directly
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                walk(child, source, file, class_target, result);
            }
        }
        "variable_statement"
        | "lexical_declaration" => extract_variable_decl(node, source, file, result),
        _ => recurse(node, source, file, class_target, result),
    }
}

fn recurse(
    node:         Node,
    source:       &[u8],
    file:         &str,
    class_target: Option<&str>,
    result:       &mut ExtractionResult,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, source, file, class_target, result);
    }
}

fn extract_import(node: Node, source: &[u8], file: &str, result: &mut ExtractionResult) {
    // `import { x } from './path'` — source is the string literal
    let source_str = node
        .child_by_field_name("source")
        .map(|n| text(n, source).trim_matches('"').trim_matches('\'').to_string())
        .unwrap_or_default();

    if source_str.is_empty() {
        return;
    }

    let from_id = format!("{file}::__module__");
    let to_id   = format!("{source_str}::__module__");

    result.edges.push(GraphEdge {
        from_id,
        to_id,
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

    let node_type = if class_target.is_some() { NodeType::Method } else { NodeType::Function };
    let id = node_id(file, &name);

    result.nodes.push(NodeData {
        id: id.clone(),
        label: name.clone(),
        node_type,
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

fn extract_arrow(node: Node, source: &[u8], file: &str, result: &mut ExtractionResult) {
    // Arrow functions are named only when they appear as `const Foo = () => ...`
    // The name lives in the parent variable_declarator, not the arrow_function node.
    // We handle this in extract_variable_decl instead; here we skip unnamed arrows.
    let _ = (node, source, file, result);
}

fn extract_class(node: Node, source: &[u8], file: &str, result: &mut ExtractionResult) {
    let name = match node.child_by_field_name("name") {
        Some(n) => text(n, source),
        None    => return,
    };

    let id = node_id(file, &name);

    // Superclass → InheritsFrom edge
    if let Some(heritage) = node.child_by_field_name("heritage") {
        let parent = text(heritage, source);
        if !parent.is_empty() {
            result.edges.push(GraphEdge {
                from_id:    id.clone(),
                to_id:      format!("{file}::{parent}"),
                kind:       EdgeKind::InheritsFrom,
                confidence: EdgeConfidence::Extracted,
            });
        }
    }

    result.nodes.push(NodeData {
        id: id.clone(),
        label: name.clone(),
        node_type: NodeType::Class,
        file: file.to_string(),
        line: node.start_position().row + 1,
        community: 0,
    });

    // Walk class body with class_target set
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
    // Scan for `const Foo = () => ...` or `const Foo = function() ...`
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "variable_declarator" {
            let name_node = match child.child_by_field_name("name") {
                Some(n) => n,
                None    => continue,
            };
            let value_node = match child.child_by_field_name("value") {
                Some(v) => v,
                None    => continue,
            };
            if matches!(value_node.kind(), "arrow_function" | "function_expression") {
                let name = text(name_node, source);
                if !name.is_empty() && name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
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

// ── Helpers ───────────────────────────────────────────────────────────────────

fn text(node: Node, source: &[u8]) -> String {
    node.utf8_text(source).unwrap_or("").to_string()
}

fn node_id(file: &str, symbol: &str) -> String {
    format!("{file}::{symbol}")
}
