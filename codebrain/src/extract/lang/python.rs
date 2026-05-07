//! Tree-sitter AST walker for Python (.py) files.
//!
//! Extracted node types:
//!   - `function_definition`  → NodeType::Function (or Method inside a class)
//!   - `class_definition`     → NodeType::Class
//!   - `import_statement`     → EdgeKind::Imports
//!   - `import_from_statement`→ EdgeKind::Imports
//!
//! Python's dynamic dispatch makes call graph resolution infeasible without
//! a type analyser (mypy / pyright). Call edges are not extracted.

use tree_sitter::{Node, Parser};

use crate::extract::ExtractionResult;
use crate::types::{EdgeConfidence, EdgeKind, GraphEdge, NodeData, NodeType};

pub fn extract(source: &str, file: &str) -> ExtractionResult {
    let language = tree_sitter_python::language();
    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        tracing::warn!(file, "python extractor: failed to load grammar");
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
        "import_statement"      => extract_import(node, source, file, result),
        "import_from_statement" => extract_import_from(node, source, file, result),
        "function_definition"   => extract_function(node, source, file, class_target, result),
        "class_definition"      => extract_class(node, source, file, result),
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                walk(child, source, file, class_target, result);
            }
        }
    }
}

fn extract_import(node: Node, source: &[u8], file: &str, result: &mut ExtractionResult) {
    // `import foo` or `import foo, bar`
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "dotted_name" || child.kind() == "aliased_import" {
            let module = text(child, source);
            let base   = module.split(" as ").next().unwrap_or(&module).trim().to_string();
            // Only internal (relative) imports have a path — absolute package
            // names (e.g. `import os`) stay as symbolic edges.
            result.edges.push(GraphEdge {
                from_id:    format!("{file}::__module__"),
                to_id:      format!("{base}::__module__"),
                kind:       EdgeKind::Imports,
                confidence: EdgeConfidence::Extracted,
            });
        }
    }
}

fn extract_import_from(node: Node, source: &[u8], file: &str, result: &mut ExtractionResult) {
    // `from .foo import Bar` — module_name is the source, names are the imports
    let module = node
        .child_by_field_name("module_name")
        .map(|n| text(n, source))
        .unwrap_or_default();

    if module.is_empty() { return; }

    result.edges.push(GraphEdge {
        from_id:    format!("{file}::__module__"),
        to_id:      format!("{module}::__module__"),
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

    // Skip dunder methods other than __init__ to reduce noise
    if name.starts_with("__") && name.ends_with("__") && name != "__init__" {
        // Still recurse into the body for nested functions/classes
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                walk(child, source, file, class_target, result);
            }
        }
        return;
    }

    let node_type = if class_target.is_some() { NodeType::Method } else { NodeType::Function };
    let id        = node_id(file, &name);

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

    // Recurse into function body (may contain nested functions/classes)
    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            walk(child, source, file, None, result);
        }
    }
}

fn extract_class(node: Node, source: &[u8], file: &str, result: &mut ExtractionResult) {
    let name = match node.child_by_field_name("name") {
        Some(n) => text(n, source),
        None    => return,
    };

    let id = node_id(file, &name);

    // Base classes → InheritsFrom edges
    if let Some(args) = node.child_by_field_name("superclasses") {
        let mut cursor = args.walk();
        for base in args.children(&mut cursor) {
            let base_name = text(base, source);
            if !base_name.is_empty() && base_name != "," && base_name != "(" && base_name != ")" {
                result.edges.push(GraphEdge {
                    from_id:    id.clone(),
                    to_id:      format!("{file}::{base_name}"),
                    kind:       EdgeKind::InheritsFrom,
                    confidence: EdgeConfidence::Extracted,
                });
            }
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
    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            walk(child, source, file, Some(&name.clone()), result);
        }
    }
}

fn text(node: Node, source: &[u8]) -> String {
    node.utf8_text(source).unwrap_or("").to_string()
}

fn node_id(file: &str, symbol: &str) -> String {
    format!("{file}::{symbol}")
}
