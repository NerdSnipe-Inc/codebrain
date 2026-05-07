//! Regex-based Swift (.swift) source extractor.
//!
//! Uses the `regex` crate rather than tree-sitter because all available Swift
//! grammar crates either require tree-sitter 0.23 (incompatible with our 0.22
//! runtime) or pull in tree-sitter 0.21 via `links = "tree-sitter"` which
//! creates a linker conflict.  Regex coverage is sufficient for the
//! codebrain use-cases: type graph, blast-radius, SwiftUI detection.
//!
//! Extracted symbols
//! ─────────────────
//! `class Foo`                  → NodeType::Class
//! `struct Foo`                 → NodeType::Struct
//! `enum Foo`                   → NodeType::Struct   (closest analogue)
//! `protocol Foo`               → NodeType::Class    (protocol ≈ interface)
//! `extension Foo`              → NodeType::Struct
//! `typealias Foo`              → NodeType::Variable
//! `func foo(…)` (free)         → NodeType::Function
//! `func foo(…)` (in type body) → NodeType::Method + Contains edge
//! `init(…)` / `deinit`        → NodeType::Method   + Contains edge
//! `import Foo`                 → EdgeKind::Imports
//!
//! Conformance edges (`: Protocol, AnotherProtocol`)
//!   → EdgeKind::Implements @ Inferred(0.85)
//!   → SwiftUI `View` conformance @ Inferred(0.9)

use regex::Regex;
use std::sync::OnceLock;

use crate::extract::ExtractionResult;
use crate::types::{EdgeConfidence, EdgeKind, GraphEdge, NodeData, NodeType};

// ── Compiled patterns (static, compiled once) ─────────────────────────────────

macro_rules! re {
    ($name:ident, $pat:expr) => {
        fn $name() -> &'static Regex {
            static R: OnceLock<Regex> = OnceLock::new();
            R.get_or_init(|| Regex::new($pat).unwrap())
        }
    };
}

re!(re_import,      r"^\s*import\s+(\S+)");
re!(re_type_decl,   r"^\s*(?:public\s+|open\s+|internal\s+|private\s+|fileprivate\s+|final\s+|@\w+\s+)*\b(class|struct|enum|protocol|extension)\s+(\w+)(?:\s*:\s*([^\{]+?))?(?:\s*\{|$)");
re!(re_typealias,   r"^\s*(?:public\s+|internal\s+|private\s+|fileprivate\s+)*typealias\s+(\w+)");
re!(re_func,        r"^\s*(?:public\s+|open\s+|internal\s+|private\s+|fileprivate\s+|override\s+|static\s+|class\s+|mutating\s+|@\w+(?:\([^)]*\))?\s+)*func\s+(\w+)");
re!(re_init,        r"^\s*(?:public\s+|internal\s+|private\s+|fileprivate\s+|required\s+|convenience\s+|override\s+)*\binit\b");
re!(re_deinit,      r"^\s*\bdeinit\b");
re!(re_open_brace,  r"\{");

// ── Public entry point ────────────────────────────────────────────────────────

pub fn extract(source: &str, file: &str) -> ExtractionResult {
    let mut result   = ExtractionResult::default();
    let mut stack: Vec<(String, NodeType)> = Vec::new(); // (type_name, kind) — tracks nesting
    let mut brace_depth: Vec<usize>        = Vec::new(); // depth at which each container opened
    let mut depth: usize                   = 0;

    for (lineno_0, raw_line) in source.lines().enumerate() {
        let lineno = lineno_0 + 1; // 1-indexed

        // Track brace depth (crude but good enough for well-formed Swift).
        // We count `{` and `}` on each line BEFORE and AFTER processing the
        // semantic content so that the container context is correct for the
        // current line.
        let opens  = count_char(raw_line, '{');
        let closes = count_char(raw_line, '}');

        // Pop closed containers before we look at this line's symbols.
        for _ in 0..closes {
            if depth > 0 {
                depth -= 1;
                if let Some(&d) = brace_depth.last() {
                    if depth < d {
                        stack.pop();
                        brace_depth.pop();
                    }
                }
            }
        }

        // ── import ────────────────────────────────────────────────────────────
        if let Some(cap) = re_import().captures(raw_line) {
            let module = cap[1].to_string();
            result.edges.push(GraphEdge {
                from_id:    format!("{file}::__module__"),
                to_id:      format!("{module}::__module__"),
                kind:       EdgeKind::Imports,
                confidence: EdgeConfidence::Extracted,
            });
        }
        // ── type declaration (class / struct / enum / protocol / extension) ───
        else if let Some(cap) = re_type_decl().captures(raw_line) {
            let keyword   = &cap[1];
            let type_name = cap[2].to_string();
            let node_type = match keyword {
                "class" | "protocol" => NodeType::Class,
                _                    => NodeType::Struct, // struct, enum, extension
            };
            let id = node_id(file, &type_name);

            // Avoid duplicate nodes for extensions of the same type
            if !result.nodes.iter().any(|n| n.id == id) {
                result.nodes.push(NodeData {
                    id:        id.clone(),
                    label:     if keyword == "extension" {
                                   format!("{type_name} (extension)")
                               } else {
                                   type_name.clone()
                               },
                    node_type,
                    file:      file.to_string(),
                    line:      lineno,
                    community: 0,
                });
            }

            // Conformances from `: Protocol, AnotherProtocol`
            if let Some(conformances) = cap.get(3) {
                for proto in conformances.as_str().split(',') {
                    let proto = proto.trim().to_string();
                    if proto.is_empty() || proto.starts_with('{') {
                        continue;
                    }
                    // Strip generic params if any: "Collection<Element>" → "Collection"
                    let proto = proto.split('<').next().unwrap_or(&proto).trim().to_string();
                    let confidence = if proto == "View" {
                        EdgeConfidence::Inferred(0.9)
                    } else {
                        EdgeConfidence::Inferred(0.85)
                    };
                    result.edges.push(GraphEdge {
                        from_id:    id.clone(),
                        to_id:      format!("{file}::{proto}"),
                        kind:       EdgeKind::Implements,
                        confidence,
                    });
                }
            }

            // Push onto container stack if this line opens a body on the same line
            if re_open_brace().is_match(raw_line) {
                stack.push((type_name, node_type));
                brace_depth.push(depth + opens - 1); // depth after counting opens
            }
        }
        // ── typealias ─────────────────────────────────────────────────────────
        else if let Some(cap) = re_typealias().captures(raw_line) {
            let name = cap[1].to_string();
            result.nodes.push(NodeData {
                id:        node_id(file, &name),
                label:     name,
                node_type: NodeType::Variable,
                file:      file.to_string(),
                line:      lineno,
                community: 0,
            });
        }
        // ── func ──────────────────────────────────────────────────────────────
        else if let Some(cap) = re_func().captures(raw_line) {
            let func_name = cap[1].to_string();
            let container = stack.last().map(|(n, _)| n.as_str());
            emit_function(&func_name, func_name.clone(), lineno, file, container, &mut result);
        }
        // ── init ──────────────────────────────────────────────────────────────
        else if re_init().is_match(raw_line) {
            let container = stack.last().map(|(n, _)| n.as_str());
            emit_function("init", "init".to_string(), lineno, file, container, &mut result);
        }
        // ── deinit ────────────────────────────────────────────────────────────
        else if re_deinit().is_match(raw_line) {
            let container = stack.last().map(|(n, _)| n.as_str());
            emit_function("deinit", "deinit".to_string(), lineno, file, container, &mut result);
        }

        // Advance depth after opens
        depth += opens;
    }

    result
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn emit_function(
    sym:       &str,
    label:     String,
    lineno:    usize,
    file:      &str,
    container: Option<&str>,
    result:    &mut ExtractionResult,
) {
    if sym.is_empty() { return; }
    let node_type = if container.is_some() { NodeType::Method } else { NodeType::Function };
    let id = match container {
        Some(c) => node_id(file, &format!("{c}.{sym}")),
        None    => node_id(file, sym),
    };

    result.nodes.push(NodeData {
        id: id.clone(),
        label,
        node_type,
        file:      file.to_string(),
        line:      lineno,
        community: 0,
    });

    if let Some(c) = container {
        result.edges.push(GraphEdge {
            from_id:    node_id(file, c),
            to_id:      id,
            kind:       EdgeKind::Contains,
            confidence: EdgeConfidence::Extracted,
        });
    }
}

fn node_id(file: &str, symbol: &str) -> String {
    format!("{file}::{symbol}")
}

fn count_char(s: &str, ch: char) -> usize {
    // Only count braces outside of string literals — crude approximation:
    // skip characters inside `"..."` blocks.
    let mut count   = 0usize;
    let mut in_str  = false;
    let mut escaped = false;
    for c in s.chars() {
        if escaped          { escaped = false; continue; }
        if c == '\\'        { escaped = true;  continue; }
        if c == '"'         { in_str = !in_str; continue; }
        if in_str           { continue; }
        if c == ch          { count += 1; }
    }
    count
}
