//! Graph analysis: god nodes, knowledge gaps, surprising connections.
//!
//! These functions interpret the topology of a `CodeGraph` to surface insights
//! for the formatter and for direct agent queries.

use crate::model::CodeGraph;
use crate::types::{CallerInfo, EdgeConfidence, NodeType, RouteInfo, SymbolCallers};

/// The top most-connected nodes in the graph ("god nodes" in graphify's
/// terminology). High degree = central abstraction that many parts of the
/// codebase depend on.
///
/// Returns `(label, file, degree)` tuples sorted by degree descending.
pub fn god_nodes(graph: &CodeGraph, n: usize) -> Vec<GodNode> {
    graph
        .god_nodes(n)
        .into_iter()
        .map(|(node, degree)| GodNode {
            label:  node.label.clone(),
            file:   node.file.clone(),
            line:   node.line,
            degree,
        })
        .collect()
}

/// A high-centrality node.
#[derive(Debug)]
pub struct GodNode {
    pub label:  String,
    pub file:   String,
    pub line:   usize,
    pub degree: usize,
}

/// Nodes that have only Ambiguous incoming edges — these are concepts the
/// graph knows about but whose connections are uncertain. Useful for
/// identifying areas that need more documentation or clearer imports.
pub fn knowledge_gaps(graph: &CodeGraph) -> Vec<GapNode> {
    graph
        .ambiguous_nodes()
        .into_iter()
        .map(|node| GapNode {
            label: node.label.clone(),
            file:  node.file.clone(),
        })
        .collect()
}

#[derive(Debug)]
pub struct GapNode {
    pub label: String,
    pub file:  String,
}

/// Orphan nodes: nodes with zero edges in either direction.
/// These are often dead code, copy-paste artifacts, or files the import
/// resolver couldn't connect to the rest of the graph.
pub fn orphan_nodes(graph: &CodeGraph) -> Vec<&crate::types::NodeData> {
    use petgraph::Direction;
    graph
        .nodes
        .iter()
        .filter(|node| {
            // Skip route and schema overlay nodes — they are intentionally
            // unlinked until the heuristic Contains edges are proven
            if matches!(node.node_type, NodeType::Route | NodeType::Schema) {
                return false;
            }
            let idx = match graph.index_of(&node.id) {
                Some(i) => i,
                None    => return false,
            };
            let in_deg  = graph.inner().edges_directed(idx, Direction::Incoming).count();
            let out_deg = graph.inner().edges_directed(idx, Direction::Outgoing).count();
            in_deg == 0 && out_deg == 0
        })
        .collect()
}

/// Edges that cross community boundaries — "surprising connections" in graphify's
/// terminology.  Always empty until Louvain is implemented (all communities are 0).
pub fn surprising_connections(
    graph: &CodeGraph,
) -> Vec<SurprisingEdge> {
    graph
        .cross_community_edges()
        .into_iter()
        .map(|(from, to, edge)| SurprisingEdge {
            from_label: from.label.clone(),
            from_file:  from.file.clone(),
            to_label:   to.label.clone(),
            to_file:    to.file.clone(),
            confidence: edge.confidence,
        })
        .collect()
}

#[derive(Debug)]
pub struct SurprisingEdge {
    pub from_label: String,
    pub from_file:  String,
    pub to_label:   String,
    pub to_file:    String,
    pub confidence: EdgeConfidence,
}

/// Find all callers of a symbol by label search, walking `Calls` edges in the
/// knowledge graph up to `max_depth` hops.
///
/// `query` is matched case-insensitively against node labels. The best match
/// (highest incoming degree among candidates) is used as the target. Returns
/// `None` if no node matches or the graph has no call edges.
pub fn symbol_callers(
    graph:     &CodeGraph,
    query:     &str,
    max_depth: usize,
    routes:    &[RouteInfo],
) -> Option<SymbolCallers> {
    use std::collections::HashSet;

    // Find candidate nodes by label (case-insensitive substring match).
    let q = query.to_lowercase();
    let mut candidates: Vec<_> = graph
        .inner()
        .node_indices()
        .filter_map(|idx| {
            let node = graph.node_at(idx)?;
            if node.label.to_lowercase().contains(&q) { Some(idx) } else { None }
        })
        .collect();

    if candidates.is_empty() {
        return None;
    }

    // Pick the candidate with the highest total degree as the best match.
    use petgraph::Direction;
    candidates.sort_by_key(|&idx| {
        let inner = graph.inner();
        std::cmp::Reverse(
            inner.edges_directed(idx, Direction::Incoming).count()
          + inner.edges_directed(idx, Direction::Outgoing).count()
        )
    });
    let target_idx = candidates[0];
    let target = graph.node_at(target_idx)?;

    let caller_nodes = graph.callers_bfs(&[target_idx], max_depth);

    let mut callers: Vec<CallerInfo> = caller_nodes
        .iter()
        .filter_map(|&(idx, depth)| {
            let node = graph.node_at(idx)?;
            Some(CallerInfo {
                id:    node.id.clone(),
                label: node.label.clone(),
                file:  node.file.clone(),
                line:  node.line,
                depth,
            })
        })
        .collect();
    callers.sort_by(|a, b| a.depth.cmp(&b.depth).then(a.file.cmp(&b.file)));

    let affected_files: Vec<String> = {
        let mut seen: HashSet<&str> = HashSet::new();
        let mut files: Vec<String> = callers
            .iter()
            .filter(|c| seen.insert(&c.file))
            .map(|c| c.file.clone())
            .collect();
        // Also include the target file itself.
        if seen.insert(&target.file) {
            files.insert(0, target.file.clone());
        }
        files.sort();
        files
    };

    // Overlay routes that are served by affected files.
    let _affected_routes: Vec<&RouteInfo> = routes
        .iter()
        .filter(|r| affected_files.contains(&r.file))
        .collect();

    Some(SymbolCallers {
        target_id:      target.id.clone(),
        target_label:   target.label.clone(),
        target_file:    target.file.clone(),
        target_line:    target.line,
        callers,
        affected_files,
        depth: max_depth,
    })
}

/// Quick stats used by the formatter's graph section header.
#[derive(Debug)]
pub struct GraphStats {
    pub node_count:      usize,
    pub edge_count:      usize,
    pub function_count:  usize,
    pub route_count:     usize,
    pub schema_count:    usize,
    pub community_count: usize,
}

pub fn graph_stats(graph: &CodeGraph) -> GraphStats {
    let communities: std::collections::HashSet<u32> =
        graph.nodes.iter().map(|n| n.community).collect();

    GraphStats {
        node_count:      graph.node_count(),
        edge_count:      graph.edge_count(),
        function_count:  graph.nodes_of_type(NodeType::Function).len()
                       + graph.nodes_of_type(NodeType::Method).len(),
        route_count:     graph.nodes_of_type(NodeType::Route).len(),
        schema_count:    graph.nodes_of_type(NodeType::Schema).len(),
        community_count: if communities.len() <= 1 { 0 } else { communities.len() },
    }
}
