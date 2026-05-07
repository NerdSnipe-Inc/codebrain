//! Tree-sitter AST walker for Rust source files.
//!
//! Extracted node types:
//!   - `fn` items           → NodeType::Function
//!   - `struct` items       → NodeType::Struct
//!   - `impl` blocks        → NodeType::Struct (attached to the impl target)
//!   - `mod` declarations   → NodeType::Module
//!   - `use` declarations   → EdgeKind::Imports edges
//!
//! Call edges (EdgeKind::Calls) are extracted as EdgeConfidence::Inferred
//! because Rust's fully-qualified call syntax (`Foo::bar(x)` vs `x.bar()`)
//! requires type inference to resolve definitively — we extract the callee
//! symbol name and mark it inferred.
//!
//! Method containment (EdgeKind::Contains from impl target → method) is
//! extracted as EdgeConfidence::Extracted when the impl target name is known.

use tree_sitter::{Node, Parser};

use crate::extract::ExtractionResult;
use crate::types::{EdgeConfidence, EdgeKind, GraphEdge, NodeData, NodeType};

pub fn extract(source: &str, file: &str) -> ExtractionResult {
    let language = tree_sitter_rust::language();
    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        tracing::warn!(file, "rust extractor: failed to load tree-sitter-rust grammar");
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

/// Recursively walk the AST.
/// `impl_target` carries the enclosing impl's type name so methods can
/// emit EdgeKind::Contains edges.
fn walk(
    node:       Node,
    source:     &[u8],
    file:       &str,
    impl_target: Option<&str>,
    result:     &mut ExtractionResult,
) {
    match node.kind() {
        "function_item" => extract_function(node, source, file, impl_target, result),
        "struct_item"   => extract_struct(node, source, file, result),
        "impl_item"     => extract_impl(node, source, file, result),
        "mod_item"      => extract_mod(node, source, file, result),
        "use_declaration" => extract_use(node, source, file, result),
        _ => recurse(node, source, file, impl_target, result),
    }
}

fn recurse(
    node:        Node,
    source:      &[u8],
    file:        &str,
    impl_target: Option<&str>,
    result:      &mut ExtractionResult,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, source, file, impl_target, result);
    }
}

fn extract_function(
    node:        Node,
    source:      &[u8],
    file:        &str,
    impl_target: Option<&str>,
    result:      &mut ExtractionResult,
) {
    let name = match node.child_by_field_name("name") {
        Some(n) => text(n, source),
        None    => return,
    };

    let node_type = if impl_target.is_some() { NodeType::Method } else { NodeType::Function };
    let id        = node_id(file, &name);
    let line      = node.start_position().row + 1;

    result.nodes.push(NodeData {
        id: id.clone(),
        label: name.clone(),
        node_type,
        file: file.to_string(),
        line,
        community: 0,
    });

    // Contains edge: impl target → method
    if let Some(target) = impl_target {
        let target_id = node_id(file, target);
        result.edges.push(GraphEdge {
            from_id:    target_id,
            to_id:      id.clone(),
            kind:       EdgeKind::Contains,
            confidence: EdgeConfidence::Extracted,
        });
    }

    // Walk function body for call expressions
    if let Some(body) = node.child_by_field_name("body") {
        extract_calls(body, source, file, &id, result);
    }
}

fn extract_struct(node: Node, source: &[u8], file: &str, result: &mut ExtractionResult) {
    let name = match node.child_by_field_name("name") {
        Some(n) => text(n, source),
        None    => return,
    };

    result.nodes.push(NodeData {
        id:        node_id(file, &name),
        label:     name,
        node_type: NodeType::Struct,
        file:      file.to_string(),
        line:      node.start_position().row + 1,
        community: 0,
    });
}

fn extract_impl(node: Node, source: &[u8], file: &str, result: &mut ExtractionResult) {
    // `impl Foo` or `impl Trait for Foo` — extract the self type name
    let target_name = node
        .child_by_field_name("type")
        .map(|n| text(n, source))
        .unwrap_or_default();

    if target_name.is_empty() {
        recurse(node, source, file, None, result);
        return;
    }

    // Impl for an external trait (`impl SomeTrait for Foo`) also produces an
    // Implements edge if the trait name is known.
    if let Some(trait_node) = node.child_by_field_name("trait") {
        let trait_name = text(trait_node, source);
        if !trait_name.is_empty() {
            result.edges.push(GraphEdge {
                from_id:    node_id(file, &target_name),
                to_id:      format!("{}::{}", file, trait_name),
                kind:       EdgeKind::Implements,
                confidence: EdgeConfidence::Inferred(0.8),
            });
        }
    }

    // Walk impl body with impl_target set so methods get Contains edges
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, source, file, Some(&target_name.clone()), result);
    }
}

fn extract_mod(node: Node, source: &[u8], file: &str, result: &mut ExtractionResult) {
    let name = match node.child_by_field_name("name") {
        Some(n) => text(n, source),
        None    => return,
    };

    result.nodes.push(NodeData {
        id:        node_id(file, &name),
        label:     name.clone(),
        node_type: NodeType::Module,
        file:      file.to_string(),
        line:      node.start_position().row + 1,
        community: 0,
    });

    // Walk inline module bodies
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, source, file, None, result);
    }
}

fn extract_use(node: Node, source: &[u8], file: &str, result: &mut ExtractionResult) {
    // `use crate::foo::Bar` — extract as an Imports edge from the file module
    // to the referenced symbol.  We emit edges toward any non-external path
    // (crate:: / super:: / self::).
    let use_text = text(node, source);
    // Strip `use ` prefix and trailing `;`
    let path = use_text
        .trim_start_matches("pub ")
        .trim_start_matches("use ")
        .trim_end_matches(';')
        .trim();

    if path.starts_with("crate::") || path.starts_with("super::") || path.starts_with("self::") {
        let from_id = format!("{file}::__module__");
        let to_id   = format!("{file}::{path}");
        result.edges.push(GraphEdge {
            from_id,
            to_id,
            kind:       EdgeKind::Imports,
            confidence: EdgeConfidence::Extracted,
        });
    }
}

/// Walk a function/block body looking for call expressions and emit
/// EdgeKind::Calls edges (Inferred — call targets need type resolution).
fn extract_calls(node: Node, source: &[u8], file: &str, caller_id: &str, result: &mut ExtractionResult) {
    if node.kind() == "call_expression" {
        if let Some(func_node) = node.child_by_field_name("function") {
            let callee = text(func_node, source);
            // Strip path qualifiers to get the leaf symbol name
            let leaf = callee.split("::").last().unwrap_or(&callee).trim();
            if !leaf.is_empty() && leaf.chars().next().map(|c| c.is_alphabetic()).unwrap_or(false) {
                result.edges.push(GraphEdge {
                    from_id:    caller_id.to_string(),
                    to_id:      format!("{file}::{leaf}"),
                    kind:       EdgeKind::Calls,
                    confidence: EdgeConfidence::Inferred(0.7),
                });
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_calls(child, source, file, caller_id, result);
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn text(node: Node, source: &[u8]) -> String {
    node.utf8_text(source).unwrap_or("").to_string()
}

fn node_id(file: &str, symbol: &str) -> String {
    format!("{file}::{symbol}")
}
