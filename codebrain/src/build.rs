//! Assembles the `CodeGraph` from all extraction sources.
//!
//! The build pipeline merges three data streams into one `CodeGraph`:
//!
//! 1. **AST results** — `ExtractionResult`s from `extract::ast`.
//!    Function/struct/class nodes and import/call/contains edges with
//!    `EdgeConfidence::Extracted`.
//!
//! 2. **Detector overlays** — `RouteInfo` and `SchemaModel` from the existing
//!    regex detectors converted to `NodeType::Route` and `NodeType::Schema`
//!    nodes. These are inserted after AST nodes so they can share the same
//!    graph and be connected to handler functions.
//!
//! 3. **Semantic results** — `ExtractionResult`s from `extract::semantic`
//!    (empty in v1; merged transparently when non-empty).
//!
//! The merge order matters: AST nodes are added first. If a detector produces
//! a Route node whose file already has a function node of the same name, the
//! Route is added as a separate node and a `Contains` edge connects the
//! handler function to the route (heuristic, Inferred confidence).

use crate::extract::ExtractionResult;
use crate::model::CodeGraph;
use crate::types::{
    EdgeConfidence, EdgeKind, GraphEdge, GraphStore, NodeData, NodeType,
    RouteInfo, SchemaModel,
};

/// Build a `CodeGraph` from all extraction sources.
///
/// `ast_results` and `semantic_results` are (relative_path, ExtractionResult)
/// pairs. `routes` and `schemas` come from the existing detector pass.
pub fn build(
    ast_results:      Vec<(String, ExtractionResult)>,
    semantic_results: Vec<(String, ExtractionResult)>,
    routes:           &[RouteInfo],
    schemas:          &[SchemaModel],
) -> CodeGraph {
    let mut store = GraphStore::default();

    // ── 1. Merge AST extraction results ──────────────────────────────────────
    for (_file, result) in ast_results {
        store.nodes.extend(result.nodes);
        store.edges.extend(result.edges);
    }

    // ── 2. Merge semantic extraction results (empty in v1) ───────────────────
    for (_file, result) in semantic_results {
        store.nodes.extend(result.nodes);
        store.edges.extend(result.edges);
    }

    // ── 3. Overlay Route nodes from detector output ───────────────────────────
    for route in routes {
        let node_id = route_node_id(route);
        let label   = format!("{} {}", route.method, route.path);

        store.nodes.push(NodeData {
            id:        node_id.clone(),
            label,
            node_type: NodeType::Route,
            file:      route.file.clone(),
            line:      0,   // routes have no single line — they span a handler
            community: 0,
        });

        // Heuristic: connect the route node to any function node in the same
        // file whose name appears in the path tags. This is Inferred because
        // we cannot guarantee which function handles the route without the
        // framework's router registry.
        for tag in &route.tags {
            let candidate_id = format!("{}::{}", route.file, tag);
            store.edges.push(GraphEdge {
                from_id:    node_id.clone(),
                to_id:      candidate_id,
                kind:       EdgeKind::Contains,
                confidence: EdgeConfidence::Inferred(0.6),
            });
        }
    }

    // ── 4. Overlay Schema nodes from detector output ──────────────────────────
    for schema in schemas {
        let node_id = schema_node_id(schema);

        store.nodes.push(NodeData {
            id:        node_id,
            label:     schema.name.clone(),
            node_type: NodeType::Schema,
            file:      String::new(), // schemas span multiple files; file is ambiguous
            line:      0,
            community: 0,
        });
    }

    // ── 5. Deduplicate nodes (last-writer-wins by id) ─────────────────────────
    dedup_nodes(&mut store.nodes);

    CodeGraph::from_store(store)
}

/// Stable node ID for a route: encodes method + path to be unique.
pub fn route_node_id(route: &RouteInfo) -> String {
    format!(
        "{}::route::{}::{}",
        route.file,
        route.method.to_uppercase(),
        route.path
    )
}

/// Stable node ID for a schema model.
pub fn schema_node_id(schema: &SchemaModel) -> String {
    let table = schema
        .table_name
        .as_deref()
        .unwrap_or(&schema.name);
    format!("schema::{}", table.to_lowercase())
}

/// Remove duplicate nodes, keeping the last occurrence of each id.
/// "Last wins" means detector overlays (added last) take precedence over
/// placeholder nodes emitted by the AST walker for unresolved symbols.
fn dedup_nodes(nodes: &mut Vec<NodeData>) {
    let mut seen = std::collections::HashMap::<String, usize>::new();
    for (i, node) in nodes.iter().enumerate() {
        seen.insert(node.id.clone(), i);
    }
    // Keep only nodes at the winning indices, preserving order.
    let mut winning: std::collections::HashSet<usize> = seen.into_values().collect();
    let mut i = 0;
    nodes.retain(|_| {
        let keep = winning.remove(&i);
        i += 1;
        keep
    });
}
